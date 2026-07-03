#!/usr/bin/env bash
# Build the static GitHub Pages site that hosts the Windows update feed (issue #131).
#
# Output layout (served at https://befeast.github.io/ok-player/):
#   index.html                     the landing page (from docs/site)
#   .nojekyll                      serve files verbatim (no Jekyll pass over the JSON)
#   updates/win/releases.win.json  the Velopack win-channel manifest, rewritten so every
#                                  Assets[].FileName is the absolute GitHub release-asset URL
#
# The manifest is derived, never authored: the newest published v* GitHub release that
# carries releases.win.json is the source of truth (the same discovery rule the #130 bridge
# in release-linux.yml uses), so every run is idempotent — triggers can race or repeat and
# the last deploy still serves the newest feed.
#
# Requires: gh (authenticated; GH_TOKEN in CI), jq. Fails loudly rather than publish a wrong
# feed: a missing feed release, an empty manifest, or a referenced package that is not
# attached to the carrying release all abort — a dead download link would strand the fleet
# mid-update, and #131 exists because silent feed loss already blinded the fleet once.

set -euo pipefail

repo="${1:?usage: build-win-feed.sh <owner/repo> <output-dir>}"
out="${2:?usage: build-win-feed.sh <owner/repo> <output-dir>}"
site_src="$(cd -- "$(dirname -- "$0")/.." && pwd)/docs/site"

# --- Stage the static site skeleton --------------------------------------------------
mkdir -p "${out}"
cp -a "${site_src}/." "${out}/"
touch "${out}/.nojekyll"

# --- Locate the newest Windows release that carries the feed -------------------------
# v[0-9]* tags only (never linux-v*), newest by published_at, must carry
# releases.win.json — bridged copies on linux-v* releases are deliberately excluded so
# a stale copy can never shadow a newer Windows release.
win_tag="$(gh api --paginate "repos/${repo}/releases?per_page=100" \
  --jq '.[] | select((.draft | not) and .published_at != null)
            | select(.tag_name | test("^v[0-9]"))
            | select(any(.assets[]; .name == "releases.win.json"))
            | [.published_at, .tag_name] | @tsv' \
  | sort | tail -n 1 | cut -f 2)"
if [[ -z "${win_tag}" ]]; then
  echo "::error::No published v* release carries releases.win.json; refusing to build a feed from nothing (issue #131)."
  exit 1
fi
echo "Deriving the win feed from release ${win_tag}"

# --- Rewrite the manifest: FileName -> absolute release-asset URL --------------------
# SimpleWebSource downloads URL-valued FileNames as-is (pinned by UpdateFeedTests), so
# packages stay on GitHub release assets; only discovery moves to Pages (whose 100 MB
# file cap rules out hosting the packages themselves). Resolution is strict against the
# carrying release: GithubSource semantics already pinned downloads to the carrying
# release, so a feed entry without a co-located asset was broken for the installed
# fleet before this script ever saw it — refuse to republish it.
feed_src="$(mktemp)"
gh release download "${win_tag}" --repo "${repo}" \
  --pattern "releases.win.json" --output "${feed_src}" --clobber

asset_urls="$(gh api "repos/${repo}/releases/tags/${win_tag}" \
  --jq '[.assets[] | {(.name): .browser_download_url}] | add // {}')"

mkdir -p "${out}/updates/win"
jq --argjson urls "${asset_urls}" '
  if (.Assets | length) == 0 then
    error("the manifest lists no packages; refusing to publish an empty feed")
  else . end
  | .Assets |= map(
      if $urls[.FileName] then .FileName = $urls[.FileName]
      else error("package \(.FileName) is not attached to the carrying release") end)
' "${feed_src}" > "${out}/updates/win/releases.win.json"
rm -f "${feed_src}"

echo "Feed built from ${win_tag}:"
jq -r '.Assets[] | "  \(.Version) \(.Type)\t\(.FileName)"' "${out}/updates/win/releases.win.json"
