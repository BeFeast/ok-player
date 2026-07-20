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

Packaging also emits `portability-report.json`. It always binds the source SHA,
its embedded short build marker, and both artifact hashes to a native
dependency-equivalence pass: every dynamic ELF in the package-private library
directories must resolve either inside the bundle or to a package named by the
Debian `Depends` field, and both extracted executables must contain the expected
build marker. A candidate builder with Docker or Podman additionally records
clean Debian testing and Ubuntu 26.04 `ldd` passes, glibc and target-desktop
library rejection, source-marker checks, and canonical real-media narrow-width
and bright-video fullscreen render smokes. Public release
preparation never relies on runtime availability from the candidate host: the
hosted runner reruns the exact downloaded candidate in strict container mode
before publication. Publication rejects a missing report, the historical
narrow-width-only checklist, or a report whose identity differs from the
downloaded candidate. These gates prove that the packaged media runtime is not
borrowing undeclared build-host libraries and that decoded video remains
visible through X11 fullscreen; they do not replace installed or live operator
acceptance.

The acceptance template records a privacy-preserving SHA-256 for the artifact
build execution. Every PASS `installed-package` row must add
`execution_environment_sha256` for the independent QA execution. The validator
rejects a missing, malformed, or matching fingerprint, so `installed-launch`
cannot be credited on the execution that built the package. Derive the QA value
from a sanitized run identifier and environment description, for example:

```bash
printf '%s' 'qa-run:<run-id>:ubuntu-26.04:gnome-wayland' | sha256sum
```

Do not use a hostname, machine path, account name, or machine identifier in the
seed or in public evidence.

Every issue-owned QA or acceptance outcome must also add a reviewable Markdown
record under [`docs/qa-records/`](qa-records/README.md). Generated manifests,
screenshots, packages, and full logs remain external artifacts, but the record
must bind them to the exact source and package identities by SHA-256 and link to
the complete logs. An empty traceability commit is not an acceptance record.

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
HEVC Main10 `yuv420p10le` at 60/1 or 60000/1001, and records both the native Wayland/EGL plane and
the retained `GtkGLArea` A/B path. It also starts the native backend in canonical 480×270
Mini-player geometry and requires compositor-presented frames to dominate discarded feedback at the
acceptance cadence, binding the compact transparency regression to the live child surface. On a
fractionally scaled display,
the native evidence must also
show a render target derived from the compositor's exact surface scale rather than GTK's integer
scale-factor ceiling; compare that final buffer geometry with standalone mpv in the same window
state. Native acceptance requires at least 55 completed EGL swaps per second over a continuous
15-second post-warmup window, `hwdec-current=vaapi`, zero decoder-drop growth, no sustained VO-drop
growth, and a 1x playback clock within five percent. The generated private logs and comparison JSON
are evidence artifacts, not repository fixtures.

Generate deterministic media and captures:

```bash
./scripts/run-linux-acceptance-harness.sh ok-player artifacts/linux-acceptance
```

The runner generates media, captures all fixed states, writes `xvfb-rows.json`, and exits non-zero when any canonical redline fails. First-run evidence comes from the canonical dark empty-state suite, which also verifies the light variant; the obsolete pre-recovery main-window smoke is not part of the release harness. The required state names are defined by `okp_core::acceptance_evidence::REQUIRED_XVFB_STATES`. References use public logical IDs such as `windows-player-redlines`, `history-handoff`, and `about-handoff`; local source locations must never be written into evidence.

Candidate window-fit readiness is a separate release-engineering gate. Run
`scripts/run-linux-window-fit-series.sh <binary> <output>` to execute the complete
fit-only small/maximized/fullscreen/4K smoke three consecutive times. Each run
uses a portal-free isolated X11 session with one Xfwm process owning both roots
and private XDG cache/runtime paths. It requires the previous process, every
named window, and the GTK/MPRIS/AT-SPI D-Bus names to be gone before the next
launch, and preserves PID/XID/map-state, geometry, guard, explicit-command,
app-log, Xfwm ownership, and session-bus diagnostics.
The wrapper proves the fresh bus was reachable during the command and
unreachable after teardown. Its Linux child subreaper terminates and waits for
orphaned command or D-Bus descendants before verifying that no process retains
the bus address;
the Xvfb supervisor records readiness, confirms the
server remained alive through the command, explicitly reaps it, and removes its
private display state before returning. GLX remains enabled under the pinned
Mesa software vendor so the GTK/libmpv render surface must map without loading
the host NVIDIA EGL stack. If any run fails, the command exits
non-zero and the next attempt starts a new series from zero.
This Xvfb evidence does not prove live GNOME chooser, drag/drop, clipboard,
portal, compositor, or focus behavior.

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
workarea clamps, fractional logical pointer offsets, signed inward motion, and Shift press/release
mid-drag). GTK owns resize-handle pointer motion and sends one bounded logical size request per
changed result; compositor configure sizes are observations only and never feed back into the core
session. X11 also applies the core's opposite-edge position delta. Wayland cannot position a normal
toplevel, so it uses the closest stable size-only anchor and leaves placement safety to Mutter.

Continuous, non-jittering resize and a stable aspect ratio within a small rounding tolerance are a
`gnome-wayland-operator` row and cannot be proven under Xvfb. The operator confirms, in a real
GNOME/Wayland session, that a Shift-drag from every edge/corner follows the pointer monotonically,
holds the aspect within 0.01 throughout the drag, never snaps back or drifts off-screen, and that
pressing/releasing either Shift key during the drag keeps the reached size. The same run covers
ordinary free resize, initial fit, fullscreen, maximize/restore, final-size retention, and window
drag. The X11 focused smoke is useful regression evidence for pointer projection and transitions,
but it is not evidence for Mutter placement, compositor focus, or fractional-scale behavior.

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

Linux Release build-only run
[`29705232940`](https://github.com/BeFeast/ok-player/actions/runs/29705232940)
reported portability `pass` but its exact alpha.113 Debian and AppImage payloads
failed media open on GNOME/Wayland and rendered a black fullscreen frame under
Xvfb. Its narrow-width-only portability checklist is no longer accepted, so the
run is permanently ineligible for publication. Alpha.113 must be rebuilt from
the fix and accepted as new artifact bytes.

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

## Public beta archive and historical release-object cleanup

Issue #350 cleanup is deliberately split into preparation and operator execution. Repository tools
may read GitHub and write local evidence JSON, but they never create or delete a Release, git tag,
asset, workflow run, or update feed. Run all commands from the repository root and keep the output
bundle with the release acceptance evidence:

```bash
mkdir -p artifacts/linux-release-cleanup
./scripts/linux-release-preparation.sh archive-export \
  --repository BeFeast/ok-player \
  --output artifacts/linux-release-cleanup/linux-release-archive.json
./scripts/linux-release-preparation.sh anchor-check \
  --archive artifacts/linux-release-cleanup/linux-release-archive.json \
  --tag linux-v0.1.0-linux-alpha.112 \
  --output artifacts/linux-release-cleanup/migration-anchor-check.json
./scripts/linux-release-preparation.sh cleanup-plan \
  --archive artifacts/linux-release-cleanup/linux-release-archive.json \
  --allowlist docs/linux-release-retain-allowlist.json \
  --batch-size 20 \
  --candidate-upgrade-evidence artifacts/linux-release-cleanup/candidate-upgrade-evidence.json \
  --output artifacts/linux-release-cleanup/cleanup-plan.json
```

The archive maps every `linux-v*` release object to its immutable release ID, tag source SHA,
metadata, assets, GitHub SHA-256 digests, and download URLs. It separately records every historical
`linux-v*` git tag, including tags without a Release object. Archive generation fails on a missing
tag, mismatched source SHA, duplicate release/asset identity, missing digest, or cross-tag asset URL.

The checked-in allowlist is exact. It retains `linux-v0.1.0-linux-alpha.112` as the installed
migration anchor and reserves `linux-v0.11.0-beta.1` as the surviving public beta. The anchor is
retained indefinitely unless both conditions in its `removal_gate` become true: the machine-readable
candidate-upgrade evidence passes and the operator explicitly closes the migration window. In the
absence of that explicit close, no date or elapsed time authorizes its removal. Run `anchor-check`
before cleanup and after every batch; every archived asset must still return a downloadable HTTP
status.

`cleanup-plan.json` is always `dry_run: true`. It is `execution_ready: true` only when the supplied
`CandidateUpgradeEvidence` passes the core cleanup gate and the archived release set contains the
allowlisted public beta. It lists exact GitHub Release object IDs in oldest-first bounded batches
and repeats the full retain allowlist. It never emits a git-ref deletion and records every preserved
tag. The operator must review and approve one batch at a time; a release ID not present in that
reviewed batch is outside the cleanup scope. Never use `gh release delete
--cleanup-tag`, `git push --delete`, or a tag API during this procedure.

After `0.11.0-beta.1` is published and its static feeds have refreshed, capture a feed audit before
the first cleanup batch and after each batch:

```bash
./scripts/linux-release-preparation.sh feed-audit \
  --repository BeFeast/ok-player \
  --feed-base https://befeast.github.io/ok-player \
  --expected-linux 0.11.0-beta.1 \
  --installed-linux 0.1.0-linux-alpha.112 \
  --expected-windows 0.10.14 \
  --installed-windows 0.10.13 \
  --output artifacts/linux-release-cleanup/feed-before-batch-01.json

# Run the same audit after the operator-approved batch, changing only --output.
./scripts/linux-release-preparation.sh feed-compare \
  --before artifacts/linux-release-cleanup/feed-before-batch-01.json \
  --after artifacts/linux-release-cleanup/feed-after-batch-01.json \
  --output artifacts/linux-release-cleanup/feed-comparison-batch-01.json
```

The audit verifies both Linux lanes point to the exact beta Release, the `.deb` names a co-located
`SHA256SUMS`, the Windows feed still points to the intended `v*` Release, every referenced asset is
downloadable, and the named installed predecessors select the intended newer versions. The
comparison fails unless the Linux `.deb`, Linux Velopack, and Windows feed bytes, decisions, URLs,
and asset availability are unchanged across the batch.

Retain the archive, allowlist, migration-anchor checks, dry-run plan, exact operator-approved batch
lists, API deletion responses, before/after feed audits, feed comparisons, and a final Releases-page
count as the cleanup audit. Any retained exception beyond the checked-in allowlist must be added to
that file in a reviewed repository change before a new plan is generated; never improvise an
exception while executing a batch.
