# Linux candidate channel: rolling QA updates (issue #339)

The candidate channel lets explicitly enrolled Linux QA installs update from frequent native Ubuntu builds without creating one permanent GitHub Release per checkpoint. Public beta/stable discovery remains unchanged.

## Isolation and enrollment

- The public feeds remain `updates/linux/releases.linux.json` and `updates/linux/deb.linux.json` on GitHub Pages. Candidate publication never invokes the Pages workflow or either public-feed generator.
- Candidates use one mutable pre-release tagged `linux-candidate`. It is a rolling publication surface, not a permanent product release.
- The single candidate pointer is `candidate.linux.json`. Only `updates.channel: "candidate"` or `OKP_LINUX_UPDATE_CHANNEL=candidate` fetches it; missing, unknown, and default settings remain `public`.
- The candidate AppImage and `.deb` lanes both derive from this pointer. The AppImage updater does not independently consume a mutable Velopack feed, so a partial upload cannot expose an unaccepted AppImage candidate.
- Update suppression is isolated too. `updates.skipped_versions.candidate`
  stores only the exact skipped candidate, independently of
  `updates.skipped_versions.public`. Switching enrollment cannot leak a public
  skip into candidate discovery or vice versa, and candidate N never suppresses
  candidate N+1.

## Native-builder handoff

`release-linux-candidate.yml` runs every 15 minutes on a generic self-hosted Linux x86_64 runner. Immediately after checkout, a scheduled run refreshes `origin/main`; if its requested SHA is already behind, it emits `OKP_CANDIDATE_SKIPPED_SUPERSEDED` and exits successfully before toolchain preflight or lock acquisition. One close-on-exec coordination lock covers `scripts/build-linux-candidate.sh`, the verified-bundle handoff, and `scripts/publish-linux-candidate.sh`. The builder coalesces all changes at the latest `origin/main`, skips an unchanged SHA, and emits the #340 native bundle; publication consumes that exact bundle and never rebuilds on `ubuntu-latest`.

The public Linux Release workflow still builds fresh stable-version package
bytes, but its digest-pinned Ubuntu 26.04 media builder matches this native
builder's supported dependency generation. Both paths call the same Debian and
Velopack package entry points, and both are rejected unless the exact packages
pass narrow-width plus bright-video fullscreen media smokes.

Runs that reach the builder record an `idle`, `building`, or `stalled` heartbeat summary. A superseded run stops before heartbeat state exists and records the newer head in the workflow notice and summary instead. Manual dispatch remains an operator override for republishing the last verified bundle or changing its acceptance status and deliberately bypasses the early supersession check.

The fit gate is deliberately non-publishing when run by itself. After a
lifecycle repair, validate the exact branch head with
`scripts/run-linux-window-fit-series.sh`; do not dispatch this workflow to obtain
fit evidence. Candidate publication remains controlled by the normal scheduler
or an operator after merge.

## Monotonic identities

Candidates follow the issue's SemVer ladder:

| Phase | Identity |
| --- | --- |
| before public beta 1 | `0.11.0-beta.0.<build>` |
| public beta 1 | `0.11.0-beta.1` |
| after beta 1 | `0.11.0-beta.1.<build>` |
| public beta 2 | `0.11.0-beta.2` |

The first public beta has not been published yet, so the native builder defaults
to `0.11.0-beta.0` and produces pre-beta candidates such as
`0.11.0-beta.0.42`. Once `0.11.0-beta.1` is deliberately published, the
operator sets `OKP_CANDIDATE_VERSION_BASE=0.11.0-beta.1` to produce subsequent
candidates such as `0.11.0-beta.1.43`. `okp-core` owns version construction and
monotonic comparisons. Tests cover sequential pre-beta candidate discovery,
the transition to the public `0.11.0-beta.1`, and the explicit post-beta base
change.

## Exact identity and acceptance

Every `candidate.linux.json` records:

- exact source git SHA and monotonic build number;
- UTC completion timestamp;
- `pending`, `accepted`, or `rejected` acceptance status;
- exact `.deb` name, size, URL, and SHA-256;
- exact Velopack full-package name, size, URL, SHA-256, and package identity;
- a build-versioned checksum URL (`SHA256SUMS-<build>.txt`).

Only `accepted` candidates are selected. Velopack's candidate pack names are channel-qualified:
`com.befeast.okplayer-linux-candidate.AppImage`,
`com.befeast.okplayer-<version>-linux-candidate-full.nupkg`, and
`releases.linux-candidate.json`. Packaging resolves those generated identities from the feed and
package bytes rather than guessing public-channel names. It proves that the standalone AppImage is
byte-identical to the AppImage embedded in the Full nupkg, then atomically stages the user-facing
`OK-Player-<version>-x86_64.AppImage`; a failed copy leaves no versioned partial.

Immediately before publication, `okp-candidate verify-bundle` re-reads the native bundle,
recomputes the `.deb`, AppImage, and Velopack full-package hashes, compares
`candidate-build.json` with `package-identity.json`, validates `SHA256SUMS`, and requires
`releases.linux-candidate.json` to contain exactly one matching candidate Full package. Replacing
bytes after the build therefore blocks promotion.

Each packaged binary is stamped with its install lane. A `.deb` build routes an accepted newer candidate directly to the manifest-bound Debian package and never asks Velopack whether that package exists. An AppImage build routes only through Velopack; after the candidate pointer has selected a newer version, Velopack's `NoUpdateAvailable` and `RemoteIsEmpty` outcomes are reported as distinct check failures rather than as “up to date.” Development builds use the Debian path for non-destructive local testing.

Every enrolled client fetches the mutable candidate pointer with a unique query value plus
`Cache-Control: no-cache` and `Pragma: no-cache`. This prevents a previous successful check from
pinning a later accepted generation at a shared CDN edge. The publisher separately polls the plain,
canonical pointer URL with the legacy client headers and requires its response bytes to equal the
newly uploaded manifest before it advances `last-promoted.sha`, prunes assets, or reports success.
That visibility barrier keeps already-installed predecessors safe even though they cannot gain the
new cache-busting request behavior retroactively. A timeout leaves the uploaded generation and its
immutable assets available for an exact-bundle retry, but does not claim that the generation is
promoted.

For `.deb` installs, the updater first checks that the candidate manifest's SHA matches the build-versioned `SHA256SUMS`, then verifies the downloaded bytes against that manifest. For AppImage installs, the exact manifest-bound Velopack asset is the update source; Velopack verifies its size and digest while downloading. Candidate checks log the fetched version/build/SHA, core selection, stamped install lane, Velopack result when applicable, and final route so installed-package evidence can account for every decision stage without exposing local machine paths.

## Atomic promotion

Promotion uploads immutable, versioned assets first:

1. `.deb`;
2. standalone AppImage;
3. Velopack full package;
4. `SHA256SUMS-<build>.txt`.

`candidate.linux.json` is uploaded last and is the acceptance pointer for both lanes. A failure before that final upload leaves the previous pointer, package URLs, and versioned checksum file usable. A retry is idempotent for the same source/build and may safely change only its acceptance state.

Uploading the pointer is not sufficient to complete promotion. The canonical, query-free download
URL must expose the exact uploaded bytes to an unchanged installed client. Until that check passes,
the workflow fails without updating the promoted marker; its next scheduled run reuses the same
verified versioned assets and replaces/rechecks the pointer.

## Stale-generation fence

Immediately before any rolling GitHub Release mutation, the publisher performs
one final decision under the same build/publish lock. It compares:

- the SHA that scheduled or dispatched the workflow;
- the SHA recorded by the verified bundle and all of its gates;
- the current candidate-policy head (`main`);
- the bundle generation against the newest locally allocated generation; and
- the bundle generation/SHA against the existing rolling pointer, when present.

Publication is eligible only when the requested, built, and current SHAs are
identical, no later generation has been allocated, and the rolling pointer has
no newer or conflicting generation. The remote pointer is downloaded and the
current branch head is read before release creation or asset upload.

A queued or coalesced run that fails this fence exits successfully as
`stale_generation`. Its machine-readable evidence records the requested, built,
current, and previously published SHAs and generation counters plus every stale
reason. It does not create a release, upload or delete an asset, move
`candidate.linux.json`, prune history, or update `last-promoted.sha`. A later run
whose requested SHA matches the already-built current bundle may publish that
same verified generation without rebuilding it.

The workflow handoff summary labels this result `delivery classification:
non_delivery`. Its separate `stable public feed: untouched` line describes the
permanent public update lane and must never be interpreted as evidence that the
rolling candidate pointer advanced.

A headless gate failure remains fail-safe. Workflow-dispatch run `29639207396`
failed on source `50495469570dd31129581b158678d33fb22a574d` before promotion,
while the immediately following scheduled run `29639587120` built the same
source and published accepted candidate `0.11.0-beta.0.21`. The previous
accepted pointer and assets stayed usable throughout the failed invocation.

## Retention and rollback

The manifest history stores complete previous accepted recovery points: version/build, `.deb`, Velopack full package, and versioned checksum URL. After the new pointer is live, `okp-core` computes a prune plan that keeps the current candidate plus up to five previous accepted candidates, always retaining at least two once the channel has accumulated them. Temporary migration anchors named by the installed-upgrade acceptance contract are retained in addition to that rolling window until machine-readable cleanup evidence passes. Unknown assets are not deleted.

Rollback is an operator action: republish a retained verified bundle as the current pointer, or mark a bad current bundle `rejected`. The rolling release is mutable; permanent public artifacts remain on normal `linux-v*` releases.

## Verification boundary

The core end-to-end contract test creates a native bundle fixture and proves exact source SHA → verified package identities → candidate feed → enrolled updater selection while a public-feed fixture remains byte-for-byte unchanged. One publisher regression covers a coalesced run requested on SHA A but built from current SHA B. The overlap regression starts another run A on SHA/generation A and holds it after bundle verification but before any remote read or mutation. Run B then advances the isolated head/generation and publishes the exact verified SHA B bundle once. When A resumes, it records `stale_generation` while the B pointer, versioned asset bytes and hashes, mutation log, decision evidence, updater selection, and promoted marker remain unchanged. Real GitHub asset upload/order and a live installed AppImage/`.deb` update remain operator/CI integration surfaces.

Development-delivery health is evaluated separately by the versioned,
read-only [`check-project-outcome.sh`](../scripts/check-project-outcome.sh).
That check accepts only an `accepted` pointer with complete source and package
identities whose source equals or is an ancestor of current `main`. Equal
sources remain healthy without unchanged rebuilds; an ancestor source has 120
minutes from the first unpublished `main` commit. Permanent `linux-v*` release
age remains a non-blocking release-cadence diagnostic; it cannot override a
healthy rolling QA delivery signal. See
[`project-outcome-health.md`](project-outcome-health.md) for the precise bound
and safe post-merge cutover.

The accepted pointer, not a workflow conclusion, is the delivery authority.
One green run that leaves the pointer behind requests recovery; two green runs
within two hours while it is still behind fail health. A current-tip active run
suppresses that early failure only until the unpublished-main lag threshold.
Thus a successful `stale_generation` remains a correct no-op for publication
safety and a non-delivery for hands-off recovery.

The operator procedure and cleanup evidence contract for those installed updates are in
[`linux-candidate-upgrade-acceptance.md`](linux-candidate-upgrade-acceptance.md).
