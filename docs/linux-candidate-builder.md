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

Install those Ubuntu packages, then provision the pinned Velopack CLI separately;
the `dotnet-sdk-9.0` package provides the SDK but does not install global tools:

```bash
dotnet tool install --global vpk --version 1.2.0
```

The manifest's `command-or-dotnet-tool|vpk|vpk|dotnet-sdk-9.0` row is the
native-host provisioning source for that SDK package. Portable release images
must not feed that row to the base distro package install: Debian's default
sources do not contain the Microsoft SDK, and Debian packaging does not need
Velopack. `--print-portable-debian-packages` therefore emits only the shared
media build dependencies. The portable Dockerfile uses that view for its
`media`/`deb` targets and installs .NET 9 with Microsoft's install script plus
the pinned `vpk` tool only in the `appimage` target. Package preflight follows
the same lane boundary: each package entry point explicitly selects its package
and shared media-runtime gates. Debian therefore does not require `vpk`, while
AppImage validates and probes the Velopack gate. The Debian entry point also
sets `OKP_CANDIDATE_TOOLCHAIN_REQUIRE_DOTNET_TOOLS=false`, so the shared
manifest preflight does not turn the unused `vpk` row back into a .NET
requirement. The scheduled native-builder preflight keeps the full default gate
list and default tool probes, validating both package paths plus portability
before acquiring the build lock.

The hosted stable release uses the digest-pinned Debian 13 image in
`scripts/linux-portable-builder.Dockerfile`. Debian 13 is the oldest supported
runtime, so every compiled or copied ELF object has a glibc requirement no
newer than the support floor. Cross-distro verification then runs the exact
packages in both Debian testing and Ubuntu 26.04 containers so the builder and
verification environments remain independent.

Stable and candidate packaging share the runtime collector and policy. mpv's
direct JPEG dependency is copied under a private `libokp-libjpeg.so.*` SONAME
and every bundled consumer is rewritten to that name. The Debian builder can
therefore carry `libjpeg.so.62` onto Ubuntu without shadowing Ubuntu's
`libjpeg.so.8`, while the native Ubuntu candidate applies the same rule to its
own JPEG ABI instead of maintaining a lane-specific exclusion list.

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
named by the Debian `Depends` field, and both extracted executables must carry
the expected source marker. Docker and Podman remain optional on this host; when
either is present, the strict foreign-distro render check runs as an additional
gate.

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

- **Superseded** — the SHA captured when the scheduled run was created no
  longer matches the freshly fetched `origin/main` head. The workflow emits
  `OKP_CANDIDATE_SKIPPED_SUPERSEDED` with a `superseded by <sha>, skipping`
  notice and exits successfully before toolchain preflight or lock acquisition.
  The next scheduled tick targets the newer head. Manual dispatches deliberately
  bypass this check so explicit bundle republishing keeps its existing behavior.
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
| `OKP_CANDIDATE_OUT_RETAIN` | `3` | Complete local bundle generations retained after every builder exit |
| `OKP_SCRATCH_SESSION` | unset | Unique worker/run key used to attribute reclaimable temporary roots |

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

1. For scheduled workflow runs, refresh `origin/main` immediately after the
   Actions checkout and skip successfully if the checked-out SHA is already
   superseded. This happens before preflight and before the build lock exists.
2. Mirror-fetch `origin/main` and resolve HEAD.
3. Skip if HEAD equals the last successfully built SHA (`last-built.sha`).
4. Clean clone of exactly HEAD; record the source SHA.
5. Run bounded gates, aborting on the first failure:
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
     Debian `Depends` field, and both extracted executables must carry the
     embedded source marker. If Docker or Podman is available, the same gate also
     runs clean Debian testing and Ubuntu 26.04 `ldd`, rejects bundled glibc and
     target desktop libraries, and executes the canonical real-media narrow-width
     and bright-video fullscreen render smokes for both artifacts. The hash-bound
     `portability-report.json` records which mode ran and is required for
     promotion and later public publication
   - package identity + SHA-256 verification (`SHA256SUMS`, `package-identity.json`)
   - clean install / upgrade / purge through real `dpkg` with a private
     filesystem root and package database. The smoke prefers an unprivileged
     mapped-root user namespace so maintainer scripts are chrooted. When the
     builder service cannot create that namespace, it uses dpkg's non-root,
     script-chrootless mode; the package scripts honor `DPKG_ROOT`, so desktop
     and icon caches still target only the disposable root. Both install
     layouts are re-verified against the packaged libmpv runtime. Direct local
     invocations of `smoke-linux-install-upgrade.sh` retain an extraction-only
     fallback unless `OKP_SMOKE_REAL_DPKG=1` is selected; the scheduled native
     candidate builder always selects a real dpkg lifecycle mode
   - real playback and screenshot capture from the exact Debian payload, with a bundled image and SHA-256
   - headless launch smoke (Xvfb): the idle surface once, followed by the complete
     fit-only small/maximized/fullscreen/4K lifecycle three consecutive times with
     no retry inside the gate. Each invocation uses one Xfwm process for both X
     screens, private XDG cache/runtime namespaces, a fresh session bus, explicit
     GTK/MPRIS/AT-SPI name release checks, and post-command probes proving that
     the bus and every process carrying its address are gone before the next invocation.
   - optional native-hardware smoke (only when `OKP_CANDIDATE_NATIVE_SMOKE` is set)
6. Emit the artifact bundle and check promotability.
7. On a fully promotable build, advance `last-built.sha` so the next schedule
   skips this SHA. **A gate failure exits non-zero and leaves `last-built.sha`,
   `last-promoted.sha`, and every feed untouched.**
8. On every builder exit, including a failed gate or an unchanged-SHA run, prune
   `out/` to the newest three complete bundles. The generation named by
   `last-bundle.path` is pinned in addition to that window while publication or
   an explicit retry can still reference it. Incomplete numeric generations are
   removed. `OKP_CANDIDATE_OUT_RETAIN` may raise or lower the complete-bundle
   window, but must remain a positive integer.

Packaging and smoke helpers create attributable scratch names. Worker and CI
orchestrators must set a unique `OKP_SCRATCH_SESSION` and call
`scripts/reclaim-ok-player-scratch.sh` from their success/failure teardown. The
reclaimer validates the session key, checks ownership, and removes only that
session's `ok-player-*` or `okp-*` roots; it cannot sweep another concurrent
worker. Large AppImage inspection, portability, and RPM intermediates are kept
under their disk-backed output directories instead of the system temporary
filesystem. The scheduled candidate workflow supplies the key and runs the
reclaimer in an `always()` teardown step. Other worker harnesses must provide
the same two-part contract. The repository does not require a larger tmpfs.

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
default GLArea and OSC initialize at narrow width, then uses the production
no-DRI software renderer to keep a bright decoded frame visible through the
fullscreen transition against the target desktop stack. This separates default
surface initialization from deterministic pixel evidence: Xvfb cannot reliably
capture direct GL pixels and does not prove a live compositor or hardware path.
Its source-marker check catches package identity loss in linked worktrees.
Neither mode is operator acceptance. The `installed-launch` and
`installed-package-version` rows must still be executed from a separate QA
execution that did not build the artifacts; the acceptance schema rejects
matching build/execution fingerprints.

The headless output also retains `headless-launch/fit-series/run-{1,2,3}/` and
`series-evidence.txt`. Every run records the current process ID, selected XID,
viewable map state and geometry, maximized/fullscreen guards, explicit Fit
dispatch, small/1080p/vertical/4K results, exactly one final initial-fit
configure per geometry, a map-before-launch delivery boundary, secondary-launch
single-instance presentation, clean main-window close, private XDG cache/runtime paths, clean
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
without rebuilding it. The job has a 45-minute fail-safe timeout, more than
twice the normal clean build-and-smoke duration, so a disconnected self-hosted
runner cannot hold the candidate concurrency group for the previous 90-minute
window. The `always()` summary reads the raw heartbeat first and
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
