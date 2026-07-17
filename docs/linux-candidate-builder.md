# Linux candidate builder (operator guide)

The candidate builder produces frequent installable Ubuntu candidates from
`origin/main` on a self-hosted x86_64 Ubuntu 26.04 machine, decoupled from the
public release workflow. Merges to `main` never build or publish a public
release; a candidate is a private updater candidate the candidate channel
(#339) can promote.

Host registration, credentials, and machine-specific service configuration live
outside this repository. Everything here is host-agnostic and reads its paths
from the environment.

## Delivery SLA

When `main` advances, the latest eligible SHA becomes an updater candidate
within **60–90 minutes**. Multiple merges that land in the same window are
**coalesced**: the builder always targets `origin/main` HEAD, so one candidate
covers every merge since the last successful build. An unchanged SHA is **never
rebuilt** merely to satisfy a clock.

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
| `OKP_CANDIDATE_STATE_DIR` | `${XDG_STATE_HOME:-$HOME/.local/state}/ok-player-candidate` | Persistent state, lock, heartbeats, bundles |
| `OKP_CANDIDATE_REPO_URL` | public GitHub repo | Clone source |
| `OKP_CANDIDATE_BRANCH` | `main` | Branch to track |
| `OKP_CANDIDATE_VERSION_BASE` | `0.1.0` | Version prefix |
| `OKP_CANDIDATE_NATIVE_SMOKE` | unset | Optional native-hardware smoke command; when set its evidence is **required** |
| `OKP_CANDIDATE_STALL_SECONDS` | `900` | Watchdog stall threshold (reference) |

A single-run lock (`flock` on `build.lock`) makes overlapping schedules safe:
a second invocation sees the lock, records an idle heartbeat, and exits. Two
simultaneous invocations therefore cannot publish two competing candidates.

## What a build does

1. Mirror-fetch `origin/main` and resolve HEAD.
2. Skip if HEAD equals the last successfully built SHA (`last-built.sha`).
3. Clean clone of exactly HEAD; record the source SHA.
4. Run bounded gates, aborting on the first failure:
   - `cargo fmt --all -- --check`
   - clippy with warnings denied
   - workspace tests
   - Debian and AppImage/Velopack packaging
   - package identity + SHA-256 verification (`SHA256SUMS`, `package-identity.json`)
   - clean install / upgrade / uninstall smoke in a disposable environment
   - headless launch smoke (Xvfb)
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
  `SHA256SUMS`, `package-identity.json`, and the acceptance template.

This bundle is sufficient for candidate-channel promotion by #339 **without
rebuilding**.

## Build vs promotion

Build and promotion are separate by design. The builder produces and validates
a bundle; it never moves a feed. Promotion is an explicit second step:

```bash
scripts/promote-linux-candidate.sh "$OKP_CANDIDATE_STATE_DIR/out/<build-number>"
```

`promote-linux-candidate.sh` re-validates the bundle (`okp-candidate
promotable`) and records `last-promoted.sha` under the same single-run lock, so
a non-promotable bundle can never move the feed and two promotions cannot race.
Movement of the actual updater feed is owned by #339, which consumes this same
validated bundle.

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

## Native-hardware evidence

`OKP_CANDIDATE_NATIVE_SMOKE` registers an optional native-hardware smoke whose
evidence the operator can require. When set, the command runs as a gate and its
`native-hardware-smoke` result must pass for the build to be promotable. A
headless/Xvfb build cannot attest real GPU decode, compositor, or portal
behavior; that remains the operator's live-hardware surface.
