# Linux candidate installed-upgrade acceptance

This is the installed-package migration gate for issue #347. It covers both supported Linux
self-update lanes and produces machine-readable evidence before any historical release or rolling
candidate asset is removed.

## Migration anchors

Keep these objects until `candidate-upgrade-validate` passes for both lanes:

- public release `linux-v0.1.0-linux-alpha.112`, the newest public Linux predecessor currently named
  by both public feeds;
- rolling candidate `0.11.0-beta.0.10`, the newest reproducible accepted private predecessor for
  the reported regression.

The public predecessor is an enrollment boundary: alpha.112 predates candidate-channel support, so
one explicit install is required to reach the reproducible defective candidate `.10` and persist
`updates.channel="candidate"`. That enrollment bootstrap does not count as recovery from the `.10`
regression.

Starting from the unchanged installed `.10`, use exactly one of these recovery paths:

1. `feed-side-built-in`: correct the published feed/artifact surface so unchanged `.10` discovers
   and installs the first fixed candidate through Settings; or
2. `explicit-bootstrap`: if remote correction is technically impossible, publish and document one
   fixed bootstrap package and install it explicitly.

In either case, record the first fixed build as candidate N. Candidate N -> candidate N+1 must then
complete through Settings without another manual package replacement. Merely installing a freshly
built fixed package and checking that it can see an update does not account for users stranded on
`.10`.

Known anchor artifact SHA-256 values at the start of this gate are:

| Identity | Debian installable/payload | AppImage installable | Velopack full payload |
| --- | --- | --- | --- |
| public alpha.112 | `b266c20f98220b4021d308b461270ab6c5b8117575f625dab7f0bbafee38ff28` | `114577cb85afa0a9a3625046402e86be04bca60fcba8b7d7364d4d97f3427165` | `ca82f037090a7527ca55015f9f910b9c9fee662873c66e153cbf8fa84e4232e1` |
| candidate .10 | `43d0ff73cb4373f94d7d996b2678e71135a629bd32495ed89123700375f82717` | `f02e49c8c1d8c8143a881c42ae3b010b0493e37c52023c376d9b54f17b8c1c71` | `5ece99593e78392b6ee8a1eddc4ec1d1338111e247778853878cf72769161b9f` |

Do not copy a cached hash into final evidence. Re-hash every downloaded package and every feed body
used by the run. The values above identify which historical bytes must remain available.

## End-to-end matrix

Run both rows on live GNOME/Wayland. Headless or Xvfb runs cannot attest pkexec, restart, portal,
desktop focus, or in-place AppImage replacement.

| Lane | Public enrollment and `.10` reproduction | Recovery to fixed candidate N | Candidate N+1 | Required proof |
| --- | --- | --- | --- | --- |
| Debian | install public alpha.112; record public-feed-only behavior; explicitly install `.10`; persist `updates.channel="candidate"`; reproduce `.10` against accepted `.11` | use `feed-side-built-in` if unchanged `.10` can be repaired remotely, otherwise install one documented fixed `.deb` bootstrap; verify exact feed/package identity and recovery method | apply through Settings; verify restart/manual relaunch identity, settings hash, and history hash | fixed candidates offer the verified `.deb` without consulting Velopack; checksum identity is checked before pkexec |
| AppImage | launch public alpha.112; record public-feed-only behavior; explicitly replace it with `.10`; persist `updates.channel="candidate"`; record unchanged `.10` behavior against accepted `.11` | use `feed-side-built-in` if unchanged `.10` can be repaired remotely, otherwise install one documented fixed AppImage bootstrap; verify exact feed/package identity and recovery method | apply through Settings and restart; verify exact identity, settings hash, and history hash | fixed candidates use the manifest-bound Velopack package and apply it in place |

The historical regression is unchanged `.10` checking accepted `.11`; it must be reproduced and
accounted for, but it must not be recorded as a successful `.10 -> .11` lane. Candidate N is the
first fixed build reached through the declared recovery path, and candidate N+1 is the next accepted
build applied by that fixed updater. Keep `.10` as the regression anchor until both lanes pass.

## Persistent decision-surface acceptance

For candidate N → N+1 in both package lanes, record the installed GTK behavior
on live GNOME/Wayland:

1. Let an automatic check discover N+1. Record the target version and both
   visible actions, **Update** and **Skip this version**.
2. Leave the app untouched beyond the former toast lifetime and prove the same
   surface remains keyboard reachable and actionable. Open Settings → Updates
   and prove it projects the same target and actions.
3. Choose Skip, restart the unchanged app, and prove the exact candidate remains
   suppressed automatically. Run manual **Check for updates** and prove it says
   the version was skipped and exposes **Install anyway**.
4. Publish or point to a newer accepted candidate and prove it is offered normally.
5. Exercise one failed verified-install attempt and prove the error is visible
   while Update remains retryable for the same version; then complete the real
   update successfully.
6. Switch between public and candidate enrollment with distinct skipped values
   and prove neither channel reads the other's slot.

Capture keyboard focus and a screen-reader announcement for the group and both
actions. Xvfb evidence in `docs/visuals/issue-394` is composition/state evidence
only; it does not satisfy this installed live-desktop gate.

## Failure and recovery matrix

Each ID below is mandatory in the evidence manifest:

| Evidence ID | Expected result |
| --- | --- |
| `interrupted-download-rejected` | partial file is not installed; retry starts from a safe state |
| `corrupt-checksum-rejected` | changed package bytes fail before installation |
| `feed-identity-mismatch-rejected` | candidate pointer and checksum/package identity mismatch fails closed |
| `unavailable-feed-reported-failed` | UI reports a failed check, never “up to date” |
| `pkexec-insufficient-privilege-recovery` | Debian install is not applied and remains retryable |
| `pkexec-cancelled-recovery` | cancellation is reported and remains retryable |
| `rollback-reinstall-recovery` | retained predecessor can be reinstalled, then advance again |
| `non-enrolled-install-isolated` | default/public install never fetches or offers the candidate |
| `public-feed-unchanged` | public feed SHA-256 is identical before and after the candidate run |
| `no-update-distinct-from-check-failure` | equal version is “up to date”; network/parse/empty-lane failures are not |

For each candidate N -> N+1 publication, capture both the cache-busted pointer fetched by the fixed
client and an ordinary request to the canonical pointer URL using the unchanged predecessor's
request shape. Both bodies must have the exact N+1 SHA-256, version, build, acceptance, and source
SHA before the predecessor is expected to discover the update. A publisher timeout or a stale
ordinary response is a failed publication attempt, not evidence that the installed predecessor is
up to date. Retry the exact verified bundle and retain the failed-attempt logs plus the eventual
matching pointer hashes.

Before the first upgrade, create representative settings and a sentinel playback-history entry.
Write canonical probe JSON containing the values that must survive (for example update enrollment,
appearance/subtitle preferences, media identity, resume point, and per-file preferences), then hash
the probe. Recreate and hash the same probe after candidate N+1. The validator requires exact probe
equality while allowing unrelated runtime fields such as last-opened timestamps to advance.

## Machine-readable evidence

Record one JSON document using `CandidateUpgradeEvidence` from
`okp_core::acceptance_evidence`. It contains:

- the exact public and private migration-anchor package identities;
- public feed SHA-256 before and after;
- one Debian and one AppImage lane with the public predecessor, defective `.10` identity, fixed
  candidate N, candidate N+1, feed hashes, installable hashes, updater-payload hashes, restart
  version/SHA, and before/after settings/history probe hashes;
- each lane's `feed-side-built-in` or `explicit-bootstrap` recovery path, notes identifying the exact
  recovery package/procedure, and confirmation that candidate N+1 was applied through Settings;
- every required failure/recovery check above.

Validate it from `rust/`:

```bash
CC=/usr/bin/cc cargo run -p okp-core --bin okp-acceptance-evidence -- \
  candidate-upgrade-validate --manifest /path/to/candidate-upgrade-evidence.json
```

For Debian, `installable_sha256` and `update_payload_sha256` both identify the `.deb`. For AppImage,
`installable_sha256` identifies the published AppImage while `update_payload_sha256` identifies the
manifest-bound full `.nupkg` consumed by Velopack. Do not substitute one for the other.

The validator exits non-zero for a missing lane/check, non-monotonic versions, mismatched restart
identity, changed user state, changed public feed, invalid or lane-inconsistent updater-payload
hashes, or any non-PASS row. A successful result is the cleanup authorization; screenshots, chat
messages, or a verbal operator claim are not.

Installed launch/version evidence for every fixed candidate must be collected
on a QA execution that did not build its artifacts. Use the package-bound
acceptance template's build/execution fingerprints; a matching fingerprint is
invalid even when the distro version happens to match the builder.

The issue-owned acceptance pull request must add
[`docs/qa-records/YYYY-MM-DD-issue-NNN.md`](qa-records/README.md). Record both
lane results, the exact source/candidate SHAs, every package/feed/manifest
SHA-256, the sanitized live GNOME/Wayland environment, and links to the complete
external logs. The machine-readable manifest remains an artifact; an empty
traceability commit is invalid.

## Queued-generation publication race

Release acceptance must also preserve evidence for the candidate workflow race:

1. Snapshot the rolling release asset names and hashes plus the exact
   `candidate.linux.json` body.
2. Queue run A while SHA A is the workflow-requested head.
3. Advance `main` to SHA B and queue run B without cancelling run A.
4. Hold run A after its requested SHA and verified bundle identity are captured,
   but before it reads or mutates the rolling release.
5. Let run B finish. Require exactly one eligible publication, with the pointer
   SHA/build and Debian, AppImage, Full-package, and checksum hashes matching its
   verified bundle. An enrolled updater must observe only that newest accepted
   generation.
6. Snapshot the B pointer, every release asset and hash, decision evidence, and
   promoted marker. Resume run A and require a successful `stale_generation`
   result whose evidence names requested/built SHA A, current/published SHA B,
   and all relevant generation counters. Re-snapshot the surface and require
   byte-for-byte equality with the B snapshot and no additional mutation.

The Rust workspace regression exercises the same A/B ordering with a fake
rolling release and is part of normal CI. Live acceptance still records the
real GitHub run IDs, summaries, asset hashes, and updater observation because a
local fixture cannot attest the hosted release surface.

## Non-publishing window-fit lifecycle gate

Before allowing the scheduler to build a candidate containing a headless
lifecycle repair, run the fit-only gate three separate times from clean output
directories on the exact proposed commit:

```bash
for invocation in 1 2 3; do
  OKP_WINDOW_FIT_SOURCE_SHA="$(git rev-parse HEAD)" \
    ./scripts/run-linux-window-fit-series.sh \
    ./rust/target/release/okp-linux-gtk \
    "artifacts/window-fit-invocation-${invocation}"
done
```

This command builds or publishes nothing. Every invocation contains three
independent Xvfb/D-Bus sessions and must record viewable non-`1x1` geometry for
all four cases, one Xfwm owner across both roots, private XDG cache/runtime
paths, released GTK/MPRIS/AT-SPI names, a ready session bus, `command_status=0`,
`session_bus_teardown=clean`, and `session_process_teardown=clean`. A
failed invocation invalidates the consecutive proof; restart from invocation
one after fixing the cause.
