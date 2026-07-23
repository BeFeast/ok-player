# Windows candidate channel

Issue #483 adds a rolling Windows QA lane without changing the stable Windows
release path. `.github/workflows/release-windows-candidate.yml` starts on every
push to `main`, retains a 15-minute scheduled fallback, runs on
`windows-latest`, and can also be started manually. A run reads the identity
manifest on the mutable `windows-candidate` prerelease and skips when its exact
40-character source SHA already matches `main`.

All starts share one concurrency group, and in-progress build or publication
work is never canceled. GitHub retains the newest pending start in that group;
before SDK setup, an automatic run refreshes `origin/main` and compares it with
the checked-out SHA. If a newer head already exists, the run emits
`OKP_CANDIDATE_SKIPPED_SUPERSEDED` with a `superseded by <sha>, skipping`
notice, skips every build and publication step, and completes successfully.
Manual dispatch deliberately bypasses this early check and retains the existing
coalescing decision below.

## Identity and ordering

- Velopack package id: `com.befeast.okplayer`
- Velopack channel: `win-candidate`
- Candidate version: `0.11.0-beta.0.<github.run_number>`
- Rolling release tag: `windows-candidate`
- Feed: `releases.win-candidate.json`
- Identity manifest: `candidate.windows.json`

The workflow run number is a positive, workflow-local monotonic counter. Failed
and unchanged runs may leave gaps, but a later package can never reuse or sort
before an earlier candidate version. The counter is only an ordering key for
the mutable QA lane; it does not create public release history.

Candidate packages are compiled with assembly metadata that points their
updater directly at the `windows-candidate` release and selects the
`win-candidate` manifest. Normal builds carry no override and retain the stable
`win` channel and GitHub Pages feed. The existing `v*` operator release script
and `releases.win.json` flow are unchanged.

## Bounded promotion

The hosted job performs these gates before any feed movement:

1. verify a clean checkout whose `HEAD` equals the workflow's claimed SHA and,
   for automatic runs, skip it if current `origin/main` has superseded it;
2. build the C# solution;
3. run the engine-agnostic unit suite;
4. fetch libmpv and run the Debug real-libmpv integration suite;
5. publish the self-contained WinUI app and run Velopack `pack` for the isolated
   channel;
6. validate the generated feed, package id, version, file sizes, and SHA-256
   digests in `OkPlayer.Core`;
7. re-read `main` immediately before publication and reject a stale build.

`candidate.windows.json` records the exact git SHA, monotonic build number,
version, sanitized builder identity (`github-actions/windows-latest`), UTC
timestamp, final feed SHA-256, and every uploaded artifact's size and SHA-256.
The portable contract and its tests live in `OkPlayer.Core`; PowerShell only
orchestrates build tools and ordered GitHub asset operations.

## Feed movement and rollback

Current package bytes are uploaded first, followed by the identity manifest.
`releases.win-candidate.json` is uploaded last and is the only update pointer.
If pointer replacement fails, the workflow restores the prior feed and identity
manifest. Therefore a build, test, package, validation, stale-head, or upload
failure cannot intentionally advance the candidate feed.

The final feed contains the new Full package plus the immediately previous
known-good Full package. Post-publication pruning removes only recognized
`win-candidate` assets outside that current-plus-previous set; unknown or
operator-owned release assets are never deleted. Pruning is maintenance after
the pointer is live and does not invalidate a successful promotion.

## Project outcome health

[`check-project-outcome.sh`](../scripts/check-project-outcome.sh) reports the
rolling lane as `windows-candidate-delivery`. The Rust evaluator verifies the
manifest/feed identity, exact source relation to `main`, and the shared
120-minute unpublished-main lag bound. An unchanged promoted SHA stays healthy;
two or more consecutive automatic push or scheduled failures instead report
the newest failed workflow step and count. Manual runs are excluded from that
failure streak. Before the lane has any completed automatic history or
published pointers, the row is a bootstrap warning rather than a failure.

## Acceptance boundary

The hosted Windows runner accepts compilation, unit behavior, real-libmpv
integration behavior, package production, and update-feed identity. It does not
claim GPU decode, HDR, 4K60, physical audio-device, multi-monitor, or other
hardware-bound acceptance. The parked physical Windows checkout remains manual
and untouched by this workflow.
