# Linux candidate builder (operator guide)

The candidate builder produces frequent installable Ubuntu candidates from
`origin/main` on a self-hosted x86_64 Ubuntu 26.04 machine, decoupled from the
public release workflow. Merges to `main` never build or publish a public
release; a candidate is a private updater candidate the candidate channel
(#339) can promote.

The Fedora RPM/COPR beta is intentionally a separate lane. It uses the shared
source commit and Rust gates but does not add RPM artifacts to this Ubuntu
candidate bundle or updater feed. See [`fedora-rpm.md`](fedora-rpm.md).

Host registration, credentials, and machine-specific service configuration live
outside this repository. Everything here is host-agnostic and reads its paths
from the environment.

The canonical native tool and mpv v0.40.0 development dependency contract is
[`scripts/linux-candidate-toolchain.manifest`](../scripts/linux-candidate-toolchain.manifest).
The scheduled workflow and bundled-mpv build consume that manifest directly;
this guide deliberately does not duplicate the package list. To print the
deduplicated Ubuntu package names for host provisioning:

```bash
scripts/linux-candidate-toolchain.sh --print-ubuntu-packages
```

The preflight aggregates every missing command and pkg-config module into one
failure line before the build lock is acquired. New external commands in
`build-local-mpv.sh` must use `okp_candidate_tool`. Native package and
portability scripts declare their commands with `candidate-required-tools`;
the same preflight verifies that every declared command exists in the manifest.
The build pins the upstream commit and fails rather than substituting a distro
libmpv.

The native candidate builder does not require Docker or Podman. Candidate
packages are built on the supported Ubuntu builder and receive an equivalence
gate that inspects every dynamic ELF in the Debian and AppImage private library
directories. Resolutions must stay inside the bundle or belong to a package
named by the Debian `Depends` field. Docker and Podman remain optional on this
host; when either is present, the foreign-distro launch check runs as an
additional gate.

## Delivery SLA

When `main` advances, the latest eligible SHA becomes an updater candidate
within **60–90 minutes**. Multiple merges that land in the same window are
**coalesced**: the builder always targets `origin/main` HEAD, so one candidate
covers every merge since the last successful build. An unchanged SHA is **never
rebuilt** merely to satisfy a clock.

Project outcome health allows one bounded 30-minute scheduler/publication grace
window beyond that SLA. The 120-minute clock starts at the first `main` commit
not included in the accepted candidate. An accepted candidate equal to `main`
does not expire and is never rebuilt merely because its feed timestamp is old.
The repository-owned read-only check and its precise failure contract are
documented in [`project-outcome-health.md`](project-outcome-health.md).

Schedule the builder roughly every 15 minutes. Each run either:

- **Idle** — `main` has not advanced past the last successfully built SHA. The
  builder emits an `idle` heartbeat and exits 0. This is the expected steady
  state and is not a fault.
- **Build** — `main` advanced. The builder runs the full gated pipeline
  (~45–75 minutes of clean checkout, gates, packaging, and smokes), well inside
  the 60–90 minute SLA for a 15-minute schedule.

## Entry point

```bash
scripts/build-linux-candidate.sh
```

Configuration (all optional; no host-specific value is baked in):

| Variable | Default | Purpose |
| --- | --- | --- |
| `OKP_CANDIDATE_STATE_DIR` | `${XDG_STATE_HOME:-$HOME/.local/state}/ok-player-candidate` | Persistent state, lock owner diagnostics, heartbeats, bundles |
| `OKP_CANDIDATE_REPO_URL` | public GitHub repo | Clone source |
| `OKP_CANDIDATE_BRANCH` | `main` | Branch to track |
| `OKP_CANDIDATE_VERSION_BASE` | `0.11.0-beta.0` | Candidate version base; the build number is appended |
| `OKP_CANDIDATE_NATIVE_SMOKE` | unset | Optional native-hardware smoke command; when set its evidence is **required** |
| `OKP_CANDIDATE_STALL_SECONDS` | `900` | Watchdog stall threshold, published to `stall-after-seconds` |

Until the first public beta is deliberately published, a clean invocation uses
the default base and records `0.11.0-beta.0.<build-number>` in
`candidate-build.json`. After `0.11.0-beta.1` is published, the operator moves
the rolling channel past that public identity by setting the explicit override:

```bash
OKP_CANDIDATE_VERSION_BASE=0.11.0-beta.1 scripts/build-linux-candidate.sh
```

The override changes only the candidate version base. The monotonic build
number, exact bundle identity, candidate/public feed isolation, and separate
promotion step remain unchanged.

A single-run lock (`build.lock`) makes overlapping schedules safe. The Rust
coordinator owns a close-on-exec descriptor and records its phase, process ID,
workflow run ID when available, and source SHA when known in
`build.lock.owner.json`. A second invocation reports those owner diagnostics
and is rejected or coalesced according to the entry point. Package, Xvfb, and
headless-smoke children never inherit the descriptor, so they cannot retain the
lock after their direct parent returns.

## What a build does

1. Mirror-fetch `origin/main` and resolve HEAD.
2. Skip if HEAD equals the last successfully built SHA (`last-built.sha`).
3. Clean clone of exactly HEAD; record the source SHA.
4. Run bounded gates, aborting on the first failure:
   - `cargo fmt --all -- --check`
   - clippy with warnings denied
   - workspace tests
   - pinned mpv v0.40.0 build with the embedded Wayland DMA-BUF patch; Rust
     gates and both Ubuntu package lanes use this exact library
   - native Debian and AppImage/Velopack packaging on the supported Ubuntu
     candidate builder, with no container-runtime requirement
   - extracted payload verification that `libmpv.so.2` is packaged beside the
     executable with its complete media-runtime closure, carries the embed
     options, and resolves every bundled object through `$ORIGIN`
   - runtime-independent dependency equivalence over every bundled dynamic ELF:
     each resolution must be bundle-local or owned by a package named in the
     Debian `Depends` field. If Docker or Podman is available, the same gate also
     runs clean Debian testing `ldd`, rejects bundled target desktop libraries,
     verifies the embedded source marker, and executes the canonical real-media
     narrow-width render smoke for both artifacts. The hash-bound
     `portability-report.json` records which mode ran and is required for
     promotion and later public publication
   - package identity + SHA-256 verification (`SHA256SUMS`, `package-identity.json`)
   - clean install / upgrade / uninstall smoke in a disposable environment
   - real playback and screenshot capture from the exact Debian payload, with a bundled image and SHA-256
   - headless launch smoke (Xvfb): the idle surface once, followed by the complete
     fit-only small/maximized/fullscreen/4K lifecycle three consecutive times with
     no retry inside the gate. Each invocation uses one Xfwm process for both X
     screens, private XDG cache/runtime namespaces, a fresh session bus, explicit
     GTK/MPRIS/AT-SPI name release checks, and post-command probes proving that
     the bus and every process carrying its address are gone before the next invocation.
   - optional native-hardware smoke (only when `OKP_CANDIDATE_NATIVE_SMOKE` is set)
5. Emit the artifact bundle and check promotability.
6. On a fully promotable build, advance `last-built.sha` so the next schedule
   skips this SHA. **A gate failure exits non-zero and leaves `last-built.sha`,
   `last-promoted.sha`, and every feed untouched.**

The builder never tags, never creates a GitHub Release, and never moves an
updater feed. `release-linux.yml` is triggered only by a deliberate `linux-v*`
tag or manual dispatch — a candidate build does neither.

## Artifact bundle contract

Each build writes a bundle under `$OKP_CANDIDATE_STATE_DIR/out/<build-number>/`:

- `candidate-build.json` — the stable contract: schema version, channel
  (`linux-candidate`), source SHA, build number, version, start/finish
  timestamps, every gate result, and the package identity (file names +
  SHA-256). Modeled by `okp_core::candidate_build::CandidateBuild`.
- `artifacts/` — `.deb`, versioned AppImage, Velopack feed/package assets,
  `SHA256SUMS`, `package-identity.json`, `portability-report.json`, and the
  acceptance template.

The native equivalence report is sufficient for candidate promotion. Public
release preparation on the hosted runner always reruns the exact candidate
artifacts in a clean Debian testing container and rejects publication unless
that strict report passes. Its real-media Xvfb render proves that the packaged
GTK GLArea and OSC initialize against the target desktop stack, and its
source-marker check catches package identity loss in linked worktrees. Neither
mode is operator acceptance. The `installed-launch` and
`installed-package-version` rows must still be executed from a separate QA
execution that did not build the artifacts; the acceptance schema rejects
matching build/execution fingerprints.

The headless output also retains `headless-launch/fit-series/run-{1,2,3}/` and
`series-evidence.txt`. Every run records the current process ID, selected XID,
viewable map state and geometry, maximized/fullscreen guards, explicit Fit
dispatch, small/4K results, private XDG cache/runtime paths, clean
GTK/MPRIS/AT-SPI registration release, and fresh session-bus startup/teardown.
The bus supervisor runs as a Linux child subreaper, terminates and waits for
orphaned command or D-Bus descendants, and then verifies that no process still
retains the isolated bus address.
Xfwm is started once and must publish ownership on
both X roots before the player launches; starting one Xfwm process per screen is
invalid because each process probes every screen and the two instances race for
the same roots. The Xvfb process keeps GLX enabled under the explicitly pinned
Mesa software vendor; disabling GLX can leave GTK/libmpv's startup XID unmapped
at `1x1`, while allowing the host NVIDIA vendor can crash Xvfb. Xvfb uses
`-noreset` so removing the final client cannot enter
the multi-screen reset path, and the isolated Xvfb supervisor explicitly reaps
the disposable server instead of relying on that host-sensitive shutdown path.
A failed run aborts the series and the candidate without accepting a stale
window, increasing the readiness timeout, or retrying publication.

The failure in workflow-dispatch run `29639207396` and success in scheduled run
`29639587120` came from that Xfwm ownership race on the same source commit. The
failed ordering left the only player XID unmapped at `1x1`; the D-Bus error was
printed while the failed session was being dismantled. The next invocation
reordered the competing managers, mapped the window, and published candidate
`0.11.0-beta.0.21`. The accepted pointer remained healthy because the failed
build stopped before promotion.

This bundle is sufficient for candidate-channel promotion by #339 **without
rebuilding**. AppImage packaging is forced to the `linux-candidate` Velopack
channel. The package gate resolves the generated channel-qualified standalone
AppImage and Full nupkg from `releases.linux-candidate.json`, verifies feed
hash/size plus standalone-versus-embedded AppImage byte identity, and writes the
versioned AppImage atomically. Promotion requires that same candidate Full
package identity to match its staged bytes.

## Build vs promotion

Build and promotion are separate by design. The builder produces and validates
a bundle; it never moves a feed. Promotion is an explicit second step:

```bash
scripts/promote-linux-candidate.sh "$OKP_CANDIDATE_STATE_DIR/out/<build-number>"
```

`promote-linux-candidate.sh` re-hashes and re-validates the complete bundle
(`okp-candidate verify-bundle`) before recording `last-promoted.sha` under the
same single-run lock. The scheduled #339 workflow uses
`publish-linux-candidate.sh`, which performs the same verification, uploads the
exact bundle with the rolling pointer last, and records the promoted SHA only
after publication succeeds.

`release-linux-candidate.yml` is the repository-side schedule: every 15 minutes
it holds one `build-and-publish` critical section while the builder runs,
resolves `last-bundle.path`, and publishes that exact verified bundle. There is
no unlocked step boundary between build and publish. An unchanged `main`
remains an expected idle run; manual dispatch reuses the last verified bundle
without rebuilding it. The `always()` summary reads the raw heartbeat first and
treats the handoff outputs as optional, so an early gate failure remains the
named failure instead of causing a second summary-step error.

Before the publisher creates the rolling release or changes any asset, it
revalidates the workflow-requested SHA, bundle SHA, current `main` SHA, local
`build-number`, and existing `candidate.linux.json`. The pure-core
`okp-candidate publish-decision` command owns this policy. A mismatch is a
successful `stale_generation` no-op with machine-readable evidence in
`last-publish-decision.json` and the Actions summary. The evidence includes all
three SHAs, the bundle/newest-allocated/published generation numbers, and the
specific stale reasons.

This preserves coalescing without letting an old queued workflow publish. If
run A was requested on SHA A but starts after `main` advances to SHA B, the
builder may produce the exact SHA B bundle. Run A still stops at the publish
fence because its requested SHA is A. Run B, requested on B, may then reuse and
publish that same verified bundle. If run A built A before B landed, it stops
because A is no longer current and run B allocates the next monotonic generation
for B. No counter is reused or decremented in either case.

## Heartbeats and the watchdog

The builder appends JSON heartbeat lines to
`$OKP_CANDIDATE_STATE_DIR/heartbeat.jsonl`. An external watchdog classifies the
newest line with:

```bash
okp-candidate classify --phase <idle|building> --age-seconds <N> [--stall-after 900]
```

- **idle** — the newest heartbeat is `idle`; `main` has not advanced. Expected.
- **building** — a `building` heartbeat newer than the stall threshold; the
  build is progressing.
- **stalled** — a `building` heartbeat older than the stall threshold; the build
  is hung and needs operator attention.

This lets an operator distinguish an active build, a stalled build, and an idle
unchanged `main` without reading the full log.

The project outcome collector also examines completed scheduled runs. Two or
more consecutive failures produce the distinct blocking reason
`candidate builds failing: gate <name> (<N> consecutive)`, with the gate parsed
from the newest failed run. This reason precedes stale delivery lag so a broken
builder is not reported merely as an old candidate.

## Native-hardware evidence

`OKP_CANDIDATE_NATIVE_SMOKE` registers an optional native-hardware smoke whose
evidence the operator can require. When set, the command runs as a gate and its
`native-hardware-smoke` result must pass for the build to be promotable. A
headless/Xvfb build cannot attest real GPU decode, compositor, or portal
behavior; that remains the operator's live-hardware surface.
