#!/usr/bin/env bash
# Build the Linux portion of the static GitHub Pages update site (issue #162,
# symmetric to the Windows feed in #131).
#
# Output layout (served at https://befeast.github.io/ok-player/):
#   index.html                         the landing page (from docs/site)
#   .nojekyll                          serve files verbatim (no Jekyll pass over the JSON)
#   updates/linux/releases.linux.json  the Velopack linux-channel manifest, rewritten so every
#                                      Assets[].FileName is the absolute GitHub release-asset URL
#   updates/linux/deb.linux.json       the .deb self-update manifest: the newest linux-v*
#                                      release's ok-player_*_amd64.deb plus its SHA256SUMS URL
#
# Both artifacts are derived, never authored, from the newest published linux-v* release that
# carries releases.linux.json — so every run is idempotent and triggers can race or repeat and
# the last deploy still serves the newest feed. The site skeleton (landing page + .nojekyll) is
# shared with build-win-feed.sh; publish-update-feeds.yml runs both scripts into one _site so the
# whole site — Windows feed, Linux feeds, landing page — deploys to Pages atomically in a single
# step. The AppImage lane reads releases.linux.json through Velopack's HttpSource; the .deb lane
# reads deb.linux.json directly (okp_core::update_selection::DebFeed).
#
# Requires: gh (authenticated; GH_TOKEN in CI), jq. Fails loudly rather than publish a wrong
# feed: a missing feed release, an empty/broken Velopack manifest, a missing .deb, or a missing
# SHA256SUMS all abort — a dead download link or an unverifiable .deb would strand the fleet
# mid-update, and #162 exists because the symmetric silent feed loss already blinded Windows (#130).

set -euo pipefail

repo="${1:?usage: build-linux-feed.sh <owner/repo> <output-dir>}"
out="${2:?usage: build-linux-feed.sh <owner/repo> <output-dir>}"
site_src="$(cd -- "$(dirname -- "$0")/.." && pwd)/docs/site"

# --- Stage the static site skeleton (idempotent; also staged by build-win-feed.sh) ---
mkdir -p "${out}"
cp -a "${site_src}/." "${out}/"
touch "${out}/.nojekyll"

# --- Locate the newest Linux release that carries the Velopack feed -------------------
# linux-v* tags only, newest by published_at, must carry releases.linux.json. This is the
# single source of truth for both lanes, so the AppImage and .deb updaters always advance to
# the same version.
linux_tag="$(gh api --paginate "repos/${repo}/releases?per_page=100" \
  --jq '.[] | select((.draft | not) and .published_at != null)
            | select(.tag_name | test("^linux-v"))
            | select(any(.assets[]; .name == "releases.linux.json"))
            | [.published_at, .tag_name] | @tsv' \
  | sort | tail -n 1 | cut -f 2)"
if [[ -z "${linux_tag}" ]]; then
  echo "::error::No published linux-v* release carries releases.linux.json; refusing to build a feed from nothing (issue #162)."
  exit 1
fi
version="${linux_tag#linux-v}"
echo "Deriving the linux feed from release ${linux_tag} (version ${version})"

# --- Fetch the carrying release's assets once ----------------------------------------
release_json="$(gh api "repos/${repo}/releases/tags/${linux_tag}")"
asset_urls="$(jq '[.assets[] | {(.name): .browser_download_url}] | add // {}' <<<"${release_json}")"

mkdir -p "${out}/updates/linux"

# --- Velopack manifest: FileName -> absolute release-asset URL (mirror of build-win-feed.sh) -
# HttpSource resolves each FileName by url.join against the Pages base, so an absolute URL
# passes through verbatim and packages stay on GitHub release assets; only discovery moves to
# Pages (whose 100 MB file cap rules out hosting the packages themselves). Resolution is strict
# against the carrying release: GithubSource semantics already pinned downloads to the carrying
# release, so a feed entry without a co-located asset was broken for the installed fleet before
# this script ever saw it — refuse to republish it.
feed_src="$(mktemp)"
gh release download "${linux_tag}" --repo "${repo}" \
  --pattern "releases.linux.json" --output "${feed_src}" --clobber
jq --argjson urls "${asset_urls}" '
  if (.Assets | length) == 0 then
    error("the manifest lists no packages; refusing to publish an empty feed")
  else . end
  | .Assets |= map(
      if $urls[.FileName] then .FileName = $urls[.FileName]
      else error("package \(.FileName) is not attached to the carrying release") end)
' "${feed_src}" > "${out}/updates/linux/releases.linux.json"
rm -f "${feed_src}"

# --- .deb manifest: the newest release's ok-player_*_amd64.deb + its SHA256SUMS -------
# The .deb lane verifies the download against SHA256SUMS before handing it to pkexec
# (issue #132), so a release without the checksum manifest is unverifiable and must not be
# advertised — refuse rather than ship a null sha256sums_url the shell would only reject at
# install time.
deb_manifest="$(jq --arg version "${version}" '
  {
    version: $version,
    package: (
      [.assets[] | select(.name | test("^ok-player_.*_amd64\\.deb$"))
                 | {name, url: .browser_download_url, size}]
      | if length == 0 then
          error("release carries no ok-player_*_amd64.deb; refusing to build a .deb feed without it")
        else .[0] end),
    sha256sums_url: (
      [.assets[] | select(.name == "SHA256SUMS") | .browser_download_url]
      | if length == 0 then
          error("release carries no SHA256SUMS; refusing to advertise an unverifiable .deb (issue #132)")
        else .[0] end)
  }
' <<<"${release_json}")"
jq . <<<"${deb_manifest}" > "${out}/updates/linux/deb.linux.json"

echo "Linux Velopack feed built from ${linux_tag}:"
jq -r '.Assets[] | "  \(.Version) \(.Type)\t\(.FileName)"' "${out}/updates/linux/releases.linux.json"
echo "Linux .deb feed built from ${linux_tag}:"
jq -r '"  \(.version)\t\(.package.name)\t\(.package.url)"' "${out}/updates/linux/deb.linux.json"
