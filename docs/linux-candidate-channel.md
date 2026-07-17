# Linux candidate channel: rolling QA updates (issue #339)

The candidate channel lets explicitly enrolled Linux QA installs update from frequent native Ubuntu builds without creating one permanent GitHub Release per checkpoint. Public beta/stable discovery remains unchanged.

## Isolation and enrollment

- The public feeds remain `updates/linux/releases.linux.json` and `updates/linux/deb.linux.json` on GitHub Pages. Candidate publication never invokes the Pages workflow or either public-feed generator.
- Candidates use one mutable pre-release tagged `linux-candidate`. It is a rolling publication surface, not a permanent product release.
- The single candidate pointer is `candidate.linux.json`. Only `updates.channel: "candidate"` or `OKP_LINUX_UPDATE_CHANNEL=candidate` fetches it; missing, unknown, and default settings remain `public`.
- The candidate AppImage and `.deb` lanes both derive from this pointer. The AppImage updater does not independently consume a mutable Velopack feed, so a partial upload cannot expose an unaccepted AppImage candidate.

## Native-builder handoff

`release-linux-candidate.yml` runs every 15 minutes on a generic self-hosted Linux x86_64 runner. It invokes `scripts/build-linux-candidate.sh`, which coalesces all changes at the latest `origin/main`, skips an unchanged SHA, and emits the #340 native bundle. Publication then consumes that exact bundle with `scripts/publish-linux-candidate.sh`; it does not rebuild on `ubuntu-latest`.

The scheduled path records an `idle`, `building`, or `stalled` heartbeat summary. Manual dispatch remains an operator override for republishing the last verified bundle or changing its acceptance status.

## Monotonic identities

Candidates follow the issue's SemVer ladder:

| Phase | Identity |
| --- | --- |
| before public beta 1 | `0.11.0-beta.0.<build>` |
| public beta 1 | `0.11.0-beta.1` |
| after beta 1 | `0.11.0-beta.1.<build>` |
| public beta 2 | `0.11.0-beta.2` |

The native builder currently defaults its base to `0.11.0-beta.1`, producing post-beta-1 candidates such as `0.11.0-beta.1.42`. `okp-core` owns version construction and monotonic comparisons. Tests cover sequential candidate discovery and the transition to `0.11.0-beta.1`.

## Exact identity and acceptance

Every `candidate.linux.json` records:

- exact source git SHA and monotonic build number;
- UTC completion timestamp;
- `pending`, `accepted`, or `rejected` acceptance status;
- exact `.deb` name, size, URL, and SHA-256;
- exact Velopack full-package name, size, URL, SHA-256, and package identity;
- a build-versioned checksum URL (`SHA256SUMS-<build>.txt`).

Only `accepted` candidates are selected. Immediately before publication, `okp-candidate verify-bundle` re-reads the native bundle, recomputes the `.deb`, AppImage, and Velopack full-package hashes, compares `candidate-build.json` with `package-identity.json`, validates `SHA256SUMS`, and requires `releases.linux-candidate.json` to contain exactly one matching Full package. Replacing bytes after the build therefore blocks promotion.

For `.deb` installs, the updater first checks that the candidate manifest's SHA matches the build-versioned `SHA256SUMS`, then verifies the downloaded bytes against that manifest. For AppImage installs, the exact manifest-bound Velopack asset is the update source; Velopack verifies its size and digest while downloading.

## Atomic promotion

Promotion uploads immutable, versioned assets first:

1. `.deb`;
2. standalone AppImage;
3. Velopack full package;
4. `SHA256SUMS-<build>.txt`.

`candidate.linux.json` is uploaded last and is the acceptance pointer for both lanes. A failure before that final upload leaves the previous pointer, package URLs, and versioned checksum file usable. A retry is idempotent for the same source/build and may safely change only its acceptance state.

## Retention and rollback

The manifest history stores complete previous accepted recovery points: version/build, `.deb`, Velopack full package, and versioned checksum URL. After the new pointer is live, `okp-core` computes a prune plan that keeps the current candidate plus up to five previous accepted candidates, always retaining at least two once the channel has accumulated them. Unknown assets are not deleted.

Rollback is an operator action: republish a retained verified bundle as the current pointer, or mark a bad current bundle `rejected`. The rolling release is mutable; permanent public artifacts remain on normal `linux-v*` releases.

## Verification boundary

The core end-to-end contract test creates a native bundle fixture and proves exact source SHA → verified package identities → candidate feed → enrolled updater selection while a public-feed fixture remains byte-for-byte unchanged. Real GitHub asset upload/order and a live installed AppImage/`.deb` update remain operator/CI integration surfaces.
