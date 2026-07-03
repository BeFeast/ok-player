# Windows update feed: static HTTPS discovery (issue #131)

## Why this exists

Until v0.10.13 the Windows updater discovered releases through Velopack's `GithubSource`,
which reads exactly the first 10 entries of the GitHub release listing and silently skips
any entry without a `releases.win.json` asset. That listing is shared with the Linux track
and is not recency-ordered for this repo (#157), so ten feed-less releases were enough to
blind every installed Windows build — they reported "up to date" forever (#130). The #130
bridge in `release-linux.yml` keeps the legacy channel alive; this document describes the
structural fix: discovery moved to a static HTTPS URL that no release-list churn can bury.

## How discovery works now

- Installed builds poll **`https://befeast.github.io/ok-player/updates/win/releases.win.json`**
  (`SimpleWebSource` in `UpdateService`, channel pinned to `win`; the URL lives in
  `OkPlayer.Core.UpdateFeed`).
- The manifest's `Assets[].FileName` entries are **absolute URLs to GitHub release assets** —
  packages never move; only discovery did. `SimpleWebSource` downloads URL-valued entries
  as-is (pinned by `UpdateFeedTests` in `tests/OkPlayer.Tests`).
- A failed feed fetch **throws** inside the update check; it is never conflated with an
  empty feed. The About dialog therefore still distinguishes "couldn't check"
  (`LastCheckFailed`) from a confirmed "up to date" (`CheckedOk`).

## Who writes the feed

`.github/workflows/publish-win-feed.yml` is the only writer. It runs
`scripts/build-win-feed.sh`, which:

1. finds the newest published `v*` release carrying `releases.win.json`
   (the same discovery rule as the #130 bridge; `linux-v*` bridged copies are excluded);
2. rewrites each `FileName` to that release's asset URL, refusing to publish if the
   manifest is empty or references a package not attached to the carrying release;
3. stages it together with the `docs/site` landing page and deploys to GitHub Pages.

The deploy is atomic (the whole site swaps in one step) and every run re-derives from the
current source of truth, so repeated or racing triggers converge on the newest feed.

Triggers: `release published` for `v*` tags (fired automatically by
`installer/build-velopack.ps1 -Publish`, because `vpk upload github` publishes with the
operator's token), pushes to `main` that touch the site or the generator, and manual
`workflow_dispatch`. The feed update is part of the release pipeline — no manual step.

## Operator notes

- **One-time Pages setup:** the workflow's `configure-pages` step has `enablement: true`
  and creates the Pages site on first run. If org policy blocks API enablement, enable it
  once by hand (Settings → Pages → Source: **GitHub Actions**) and re-run the workflow.
- **Shipping a Windows release** is unchanged: `installer/build-velopack.ps1 -Publish`.
  The release event refreshes the feed; the script prints the check/fallback commands.
- **Verifying**: `curl -s https://befeast.github.io/ok-player/updates/win/releases.win.json | jq .`
  should list the just-shipped version with resolvable asset URLs.

## Transition plan (fleet still on GithubSource)

Builds ≤ v0.10.13 still discover via the GitHub release listing, so the #130/#158 bridge in
`release-linux.yml` **must stay** until the installed fleet has picked up v0.10.14 (the first
static-feed build) through the repaired legacy channel. After fleet adoption is confirmed:

1. remove the bridge steps from `release-linux.yml` (separate small PR, per #131);
2. migrate the Linux updater to the same static-feed scheme (filed as its own issue) —
   it shares the identical symmetric burial risk.
