# Linux candidate channel: rolling QA updates (issue #339)

## Why this exists

The public Linux feeds (`updates/linux/deb.linux.json`, `updates/linux/releases.linux.json`) are
each derived from the newest **published** `linux-v*` GitHub Release — one permanent Release per
build (see [update-feed.md](update-feed.md)). That model is right for products but wrong for QA
candidates: candidates are development checkpoints, and the repository has already accumulated more
than a hundred `linux-v*` release objects that were never meant to be permanent.

The candidate channel gives explicitly enrolled QA installs a way to update **frequently** without
minting a GitHub Release per build, while leaving the public beta/stable feed and its default user
behavior completely unchanged.

## Channel isolation

The candidate channel is isolated from the public feed by construction, not by convention:

- **Separate publication surface.** Candidates are published to a single, reused, **mutable**
  pre-release tagged `linux-candidate` (`release-linux-candidate.yml`), never to the GitHub Pages
  site. `scripts/build-linux-candidate-feed.sh` writes only that pre-release's assets; it never runs
  `scripts/build-linux-feed.sh` and never deploys to Pages. Therefore promoting a candidate cannot
  change a single byte of the public feed — the public feed is only ever rewritten by
  `publish-update-feeds.yml`, which this workflow does not invoke.
- **Separate feed schema and URL.** The candidate feed is `candidate.linux.json`
  (`okp_core::candidate_channel::CandidateFeed`), served from the rolling pre-release's asset URL.
  It is shaped differently from `deb.linux.json` and carries per-build provenance the public feed
  does not.
- **Explicit enrollment.** Only an install whose `Settings.updates.channel` is `candidate` — or one
  launched with `OKP_LINUX_UPDATE_CHANNEL=candidate` — ever fetches `candidate.linux.json`. Every
  default install is `public` and never touches the candidate surface. Enrollment can only *enrol*:
  an unknown channel value or a stray env value leaves the install on its persisted channel, so an
  install is never silently moved between channels.

## Monotonic build identities

Candidates use SemVer identities that sort monotonically through the transition to a public beta,
compared by `okp_core::update_selection::compare_versions`:

| Phase | Identity |
| --- | --- |
| before public beta 1 | `0.11.0-beta.0.<build>` |
| public beta 1 | `0.11.0-beta.1` |
| after beta 1 | `0.11.0-beta.1.<build>` |
| public beta 2 | `0.11.0-beta.2` |

Because selection is one version comparison, two sequential candidate builds are discovered and
applied **in order**: an install on `0.11.0-beta.0.108` takes `0.11.0-beta.0.109`, and once there it
takes the `0.11.0-beta.1` promotion — it never skips, reorders, or steps backward onto a rolled-back
candidate. `candidate_channel::tests::semver_transition_from_candidate_to_beta_one_is_monotonic` and
`two_sequential_candidate_builds_are_applied_in_order` pin this.

## Manifest provenance and acceptance gating

Every `candidate.linux.json` carries, per the contract:

- `commit_sha` — the exact git SHA the candidate was built from;
- `build` — a monotonic build number (the workflow uses `github.run_number`);
- `timestamp_utc` — an RFC 3339 UTC build timestamp;
- `package.sha256` — the artifact's SHA-256;
- `acceptance` — `pending`, `accepted`, or `rejected`.

Only an `accepted` candidate is ever offered to the fleet. A `pending` candidate can sit on the
rolling surface (visible to operators) without being pushed to enrolled installs, so acceptance
evidence can be completed before promotion. `pending`/`rejected` builds are refused by
`select_candidate_update_from_feed` even when they are newer.

## Package identity: feed/package SHA must match

`scripts/build-linux-candidate-feed.sh` refuses to publish a feed whose declared `package.sha256`
does not match the `SHA256SUMS` entry for the same file — a feed/package SHA mismatch is never
advertised. On the client, `okp_core::candidate_channel::CandidatePackage::matches_sums` rejects the
same mismatch before the download is handed to the installer, and the existing `.deb`
`SHA256SUMS` verification (issue #132) still runs on the downloaded bytes. A mismatch on either side
fails closed.

## Retention and rollback

The rolling surface keeps the **current candidate plus at least two previous known-good full
packages** for recovery:

- `candidate.linux.json`'s `history` lists the retained previous packages, newest first.
  `CandidateFeed::has_sufficient_recovery` pins the `>= 2` invariant (`MIN_RETAINED_PREVIOUS`).
- The workflow's prune step keeps exactly the packages named by the new manifest (current +
  history's `.deb` and their matching AppImages) and deletes only superseded package assets;
  `candidate.linux.json` and `SHA256SUMS` are never pruned. Retention is bounded above by
  `CANDIDATE_MAX_RETAINED` (default 5) so the surface does not grow without limit.

To roll back, an operator republishes a known-good build from `history` (or flips the current
candidate's `acceptance` to `rejected`), and enrolled installs stop advancing onto the bad build.

## Atomicity: interrupted promotion is safe

Promotion is atomic at two levels:

1. **Manifest write.** `build-linux-candidate-feed.sh` writes to a sibling temp file and `mv`s it
   over the target in one step. A crash mid-build leaves the previous `candidate.linux.json`
   untouched — never a half-written manifest.
2. **Publish order.** The workflow uploads the `.deb`, AppImage, and `SHA256SUMS` **before** it
   uploads `candidate.linux.json` (the pointer the fleet reads). If the run dies before that final
   upload, the previous `candidate.linux.json` still points at the previous, still-present package,
   so an enrolled install keeps updating to a known-good build. A simulated interrupted promotion
   therefore leaves the previous feed and package usable.

## The rolling surface is mutable — by design

Unlike a normal GitHub Release, the `linux-candidate` pre-release is **reused and rewritten** on
every candidate: assets are clobbered in place and superseded packages are pruned. It is a rolling
publication surface, not an archive. Do not link to it as a permanent artifact and do not promote it
to a full Release; the permanent, immutable artifacts live on the public `linux-v*` releases.

## Operator workflow

1. Dispatch **Linux Candidate** (`release-linux-candidate.yml`) with the candidate `version` and an
   `acceptance` of `pending`. It builds both lanes, runs the Rust gates, and publishes the candidate
   to the rolling surface as `pending`.
2. Complete acceptance evidence for that exact build (see
   [linux-release-acceptance.md](linux-release-acceptance.md)).
3. Re-dispatch with the same `version` and `acceptance: accepted` to promote it to the enrolled
   fleet. Enrolled installs pick it up on their next check; everyone else is unaffected.

## Enrolling a QA install

Set the channel once in the app's settings document (`updates.channel: "candidate"`) or launch with
`OKP_LINUX_UPDATE_CHANNEL=candidate`. Point the feed at a test surface with
`OKP_LINUX_CANDIDATE_FEED_URL` when validating the flow without touching the real rolling release.
Un-enroll by setting the channel back to `public`; the install returns to the public feed with no
other change.
