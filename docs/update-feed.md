# Update feeds: static HTTPS discovery (issues #131 Windows, #162 Linux)

## Why this exists

Until v0.10.13 the Windows updater discovered releases through Velopack's `GithubSource`,
which reads exactly the first 10 entries of the GitHub release listing and silently skips any
entry without a `releases.win.json` asset. That listing is shared with the Linux track and is
not recency-ordered for this repo (#157), so ten feed-less releases were enough to blind every
installed Windows build — they reported "up to date" forever (#130).

The Linux updater had the mirror-image fragility (#162): the AppImage lane used the same
`GithubSource` discovery window, and the `.deb` self-updater listed the first page of
`repos/BeFeast/ok-player/releases` — so a burst of Windows `v*` releases could push the newest
`linux-v*` release out of both windows and the Linux fleet would silently stop seeing updates.

Both cures are the same: discovery moved to static HTTPS URLs on GitHub Pages that no
release-list churn can bury. #131 built it for Windows; #162 did it for Linux. The #130/#158
bridge in `release-linux.yml` kept the legacy Windows `GithubSource` fleet discovering updates
during the transition; it was retired in #171 once every installed Windows client had reached the
first static-feed build (see the transition record below).

## How discovery works now

All installed builds poll a channel manifest under `https://befeast.github.io/ok-player/updates/`:

- **Windows** builds poll **`updates/win/releases.win.json`** (`SimpleWebSource` in
  `UpdateService`, channel pinned to `win`; the URL lives in `OkPlayer.Core.UpdateFeed`).
- **Linux AppImage** builds poll **`updates/linux/releases.linux.json`** (Velopack `HttpSource`,
  channel pinned to `linux`; base URL in `okp-linux-gtk` `LINUX_UPDATE_FEED_BASE_URL`, overridable
  via `OKP_LINUX_UPDATE_FEED_URL`).
- **Linux `.deb`** builds poll **`updates/linux/deb.linux.json`** (`LINUX_DEB_FEED_URL`,
  overridable via `OKP_LINUX_DEB_FEED_URL`; parsed by `okp_core::update_selection::DebFeed` and
  `select_deb_update_from_feed`). Its shape is purpose-built for the `.deb` lane and differs from
  the Velopack manifests: it names the newest release's `ok-player_*_amd64.deb` and its
  `SHA256SUMS` URL, which the shell verifies before handing the package to `pkexec` (#132).

Shared invariants across all three feeds:

- Every manifest's package entry is an **absolute URL to a GitHub release asset** — packages never
  move; only discovery did. `SimpleWebSource`/`HttpSource` download URL-valued `FileName` entries
  as-is (pinned by `UpdateFeedTests` on Windows; `HttpSource` resolves them by `url.join`).
- A **failed feed fetch throws / returns an error** inside the update check; it is never conflated
  with an empty or not-newer feed. Windows keeps the About dialog's "couldn't check"
  (`LastCheckFailed`) distinct from a confirmed "up to date" (`CheckedOk`); Linux keeps
  `LinuxUpdateStatus::Failed` ("Update check failed") distinct from `UpToDate` ("Up to date").

## Linux update decision state

Discovery and installation remain feed/lane-specific, but the user decision is
portable `okp-core` state:

- a discovered version stays in a persistent, non-modal player surface and the
  Settings → Updates page until the user chooses **Update** or **Skip this version**;
- both surfaces project the same pending package and phase; the old transient
  toast timeout does not clear the offer;
- **Update** invokes the existing verified AppImage or `.deb` path. Download/
  install failure retains the exact pending version and restores a retryable
  Update action with the error text;
- **Skip this version** writes only that exact version to
  `updates.skipped_versions.public` or `.candidate` in the human-readable
  settings JSON. The other channel and every newer version remain eligible;
- an automatic check suppresses the persistent prompt for the exact skipped
  version. A manual **Check for updates** reports that it was skipped and exposes
  **Install anyway**.

The skip is a user preference, not feed metadata. Feed generators and release
workflows therefore do not encode, upload, or infer skip state.

## Who writes the feeds

`.github/workflows/publish-update-feeds.yml` is the only writer of the Pages site. It runs both
generators into one `_site` and deploys once:

- `scripts/build-win-feed.sh` finds the newest published `v*` release carrying `releases.win.json`,
  rewrites each `FileName` to that release's asset URL (refusing to publish an empty manifest or a
  package not attached to the carrying release), and writes `updates/win/releases.win.json`.
- `scripts/build-linux-feed.sh` finds the newest published `linux-v*` release carrying
  `releases.linux.json` and derives **both** Linux artifacts from that single release: the Velopack
  manifest (`FileName` rewritten the same way) and `updates/linux/deb.linux.json` (the release's
  `.deb` plus its `SHA256SUMS` URL). It refuses to publish if the release lacks the `.deb` or the
  `SHA256SUMS` — an unverifiable `.deb` must never be advertised.

Both scripts stage the shared `docs/site` landing page idempotently, so running both into one
`_site` yields the complete site. The deploy is **atomic** (the whole site swaps in one step) and
every run re-derives from the current source of truth, so repeated or racing triggers converge on
the newest feeds — and because the deploy replaces the whole site, one workflow owning the whole
site is what keeps a Linux-triggered redeploy from clobbering the Windows feed and vice versa.

Triggers:

- **`release published`** for `v*` tags — fired automatically by `installer/build-velopack.ps1
  -Publish`, because `vpk upload github` publishes with the operator's token.
- **`workflow_call`** from `release-linux.yml` — Linux releases are created with `GITHUB_TOKEN`,
  which never fires `release published`, so `release-linux.yml` calls this workflow directly after
  it publishes a `linux-v*` release. Feed regeneration is part of the release pipeline on both
  tracks — no manual step.
- **push to `main`** touching the site or a generator, and manual **`workflow_dispatch`**.

## The Linux candidate channel (issue #339)

There is a fourth, deliberately separate Linux channel for **explicitly enrolled QA installs**: the
rolling candidate channel. It exists so QA candidates — development checkpoints, not products — can
update frequently without minting one permanent GitHub Release per build (the release list had
already accumulated more than a hundred such objects).

It is isolated from the three feeds above by construction:

- Candidates are published from the verified native-builder bundle to a single **mutable**
  pre-release tagged `linux-candidate` (`release-linux-candidate.yml`), never to the GitHub Pages
  site. The candidate publisher does not rebuild, run `build-linux-feed.sh`, or deploy Pages, so
  **the public Linux feed is byte-for-byte unaffected by candidate promotion**.
- The candidate feed is `candidate.linux.json` (`okp_core::candidate_channel::CandidateFeed`), a
  distinct schema and URL. It carries per-build provenance (git SHA, monotonic build number, UTC
  timestamp, exact `.deb` and Velopack package SHA-256, acceptance status), keeps the current plus
  at least two previous known-good packages for rollback, and uploads build-versioned packages and
  checksums before the shared `candidate.linux.json` pointer. That pointer gates both package lanes,
  so an interrupted promotion leaves the previous candidate usable.
- The scheduled native workflow holds one close-on-exec critical section across build, exact-bundle
  resolution, and candidate publication. An unchanged-SHA retry reuses `last-bundle.path`; it cannot
  silently rebuild a different commit before moving the rolling pointer.
- Candidate packing consumes Velopack's separate `releases.linux-candidate.json` build output and
  channel-qualified Full nupkg. It never rewrites or publishes the public `releases.linux.json`;
  the public package path continues to use the `linux` channel unchanged.
- **Only** an install with `Settings.updates.channel == candidate` (or `OKP_LINUX_UPDATE_CHANNEL=
  candidate`) fetches it. Every default install is `public` and never touches the candidate surface.

See [linux-candidate-channel.md](linux-candidate-channel.md) for channel isolation, retention,
rollback, and the mutable nature of the rolling surface.

## Operator notes

- **One-time Pages setup:** the workflow's `configure-pages` step has `enablement: true` and
  creates the Pages site on first run. If org policy blocks API enablement, enable it once by hand
  (Settings → Pages → Source: **GitHub Actions**) and re-run the workflow.
- **Shipping a Windows release** is unchanged: `installer/build-velopack.ps1 -Publish`. **Shipping a
  Linux release** is unchanged: push a `linux-v*` tag or dispatch `release-linux.yml`. Either
  release refreshes the feeds automatically.
- **Verifying:**
  `scripts/linux-release-preparation.sh feed-audit` validates all three manifests together,
  requires their package URLs to resolve to the expected `v*` / `linux-v*` releases, checks every
  referenced asset is downloadable, hashes the feed bytes, and records whether named installed
  predecessors select the intended versions. During historical Linux Release cleanup, capture an
  audit before and after each bounded batch and run `feed-compare`; the comparison fails on any
  Linux or Windows feed drift. See the cleanup procedure in
  [`linux-release-acceptance.md`](linux-release-acceptance.md).
- **Project outcome health:** `scripts/check-project-outcome.sh` verifies the
  current source/main CI result, the Windows static feed, and the accepted
  rolling Linux candidate's source-relative delivery lag without writing any
  feed. Permanent `linux-v*` freshness is reported separately and is
  non-blocking; see
  [`project-outcome-health.md`](project-outcome-health.md).

## Transition (complete): the legacy GithubSource bridge

While pre-migration builds were still installed they discovered updates through the GitHub release
listing rather than the static feeds, so the #130/#158 bridge in `release-linux.yml` attached the
current Windows channel assets (`releases.win.json` + the nupkgs it references) to every published
Linux release, keeping that legacy `GithubSource` fleet from being buried by release-list churn:

- **Windows:** builds ≤ v0.10.13 discovered via `GithubSource`; v0.10.14 (#161) is the first
  static-feed build (`SimpleWebSource`).
- **Linux:** builds before the #162 migration discovered the AppImage lane via `GithubSource` and
  the `.deb` lane via the releases API. The first static-feed Linux build reached them through the
  batched `linux-v*` release channel; every update after that flows through the static feed.

The bridge was retired in **#171** once every installed Windows client had reached v0.10.14: the
Windows client now reads only `SimpleWebSource` (no `GithubSource` fallback remains), so no
installed build reads the bridged assets. `release-linux.yml` no longer attaches Windows channel
assets, and the static Pages feeds are the sole discovery mechanism on both tracks. The bridged
copies left on old `linux-v*` releases are dead weight and can be deleted; the original
`releases.win.json` + nupkg assets on the `v0.10.*` Windows releases **must stay**, because the
static win feed rewrites its `FileName` entries to those releases' asset URLs.
