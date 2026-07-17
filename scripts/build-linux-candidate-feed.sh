#!/usr/bin/env bash
# Build the rolling Linux candidate feed manifest (issue #339).
#
# The public Linux feeds (scripts/build-linux-feed.sh) are derived from the newest *published*
# linux-v* GitHub Release, one permanent Release per build. That is wrong for QA candidates, which
# are development checkpoints, not products. This script assembles `candidate.linux.json`, the feed
# for an explicitly enrolled QA install, which is served from a single mutable "rolling" surface
# (one candidate at a time — see release-linux-candidate.yml) rather than a Release per build.
#
# It is deliberately pure (bash + jq, no network): the workflow builds the artifacts and uploads
# the result; this script only assembles and validates the manifest, so it is unit-testable offline.
#
# The manifest carries, per the contract:
#   channel        always "candidate" (the enrolled shell refuses any other value)
#   version        monotonic SemVer identity, e.g. 0.11.0-beta.0.108
#   build          monotonic build number
#   commit_sha     exact git SHA the candidate was built from
#   timestamp_utc  RFC 3339 UTC build time
#   acceptance     pending | accepted | rejected  (only "accepted" is offered to the fleet)
#   package        the current .deb: name, url, size, sha256
#   sha256sums_url  URL of the candidate SHA256SUMS the shell verifies the download against
#   history        previous known-good packages retained for rollback, newest first
#
# Retention: the current package plus at least CANDIDATE_MIN_RETAINED previous known-good packages
# are kept in `history` (and, by the workflow, as assets on the rolling surface) so an enrolled
# install always has something to roll back to. Package identity is enforced here: the .deb's real
# sha256 must match the SHA256SUMS entry, or the build aborts — a feed/package SHA mismatch is never
# published. The write is atomic (temp file + mv), so an interrupted run leaves the previous
# candidate manifest in place and discoverable.

set -euo pipefail

# Minimum previous known-good packages retained beside the current candidate (contract: >= 2).
CANDIDATE_MIN_RETAINED="${CANDIDATE_MIN_RETAINED:-2}"
# Upper bound on retained previous packages, so the manifest and rolling surface stay bounded.
CANDIDATE_MAX_RETAINED="${CANDIDATE_MAX_RETAINED:-5}"

version=""
build=""
commit=""
acceptance=""
deb=""
sha256sums=""
base_url=""
previous=""
output=""
timestamp="${CANDIDATE_TIMESTAMP_UTC:-}"

die() { echo "::error::$*" >&2; exit 1; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)      version="$2"; shift 2 ;;
    --build)        build="$2"; shift 2 ;;
    --commit)       commit="$2"; shift 2 ;;
    --acceptance)   acceptance="$2"; shift 2 ;;
    --deb)          deb="$2"; shift 2 ;;
    --sha256sums)   sha256sums="$2"; shift 2 ;;
    --base-url)     base_url="$2"; shift 2 ;;
    --previous)     previous="$2"; shift 2 ;;
    --output)       output="$2"; shift 2 ;;
    --timestamp)    timestamp="$2"; shift 2 ;;
    *) die "unknown argument: $1" ;;
  esac
done

for pair in "version:$version" "build:$build" "commit:$commit" "acceptance:$acceptance" \
            "deb:$deb" "sha256sums:$sha256sums" "base-url:$base_url" "output:$output"; do
  [[ -n "${pair#*:}" ]] || die "missing --${pair%%:*}"
done

case "$acceptance" in
  pending|accepted|rejected) ;;
  *) die "acceptance must be one of pending|accepted|rejected, got '$acceptance'" ;;
esac
[[ "$build" =~ ^[0-9]+$ ]] || die "build must be an integer, got '$build'"
[[ -f "$deb" ]] || die "candidate .deb not found: $deb"
[[ -f "$sha256sums" ]] || die "candidate SHA256SUMS not found: $sha256sums"

deb_name="$(basename -- "$deb")"
deb_size="$(stat -c '%s' -- "$deb")"
deb_sha_actual="$(sha256sum -- "$deb" | cut -d' ' -f1)"

# Package identity: the .deb's real digest must match the SHA256SUMS entry the shell verifies
# against. A mismatch means the manifest and the checksum manifest disagree about what the package
# is — refuse to publish it (issue #339 acceptance: "package identity rejects a feed/package SHA
# mismatch"). Reuse the same GNU sha256sum manifest format the shell parses (okp_core::sha256sums).
deb_sha_listed="$(awk -v name="$deb_name" '
  { sub(/^\*/, "", $2) }
  $2 == name { print $1; found = 1 }
  END { if (!found) exit 3 }' "$sha256sums")" \
  || die "SHA256SUMS has no entry for ${deb_name}; refusing to advertise an unverifiable candidate"
if [[ "${deb_sha_actual,,}" != "${deb_sha_listed,,}" ]]; then
  die "candidate ${deb_name} SHA mismatch: file is ${deb_sha_actual}, SHA256SUMS lists ${deb_sha_listed}"
fi

timestamp="${timestamp:-$(date -u +%Y-%m-%dT%H:%M:%SZ)}"

package_url="${base_url%/}/${deb_name}"
sums_url="${base_url%/}/SHA256SUMS"

package="$(jq -n \
  --arg name "$deb_name" \
  --arg url "$package_url" \
  --argjson size "$deb_size" \
  --arg sha256 "$deb_sha_actual" \
  '{name: $name, url: $url, size: $size, sha256: $sha256}')"

# History: fold the previous feed's current package into the front of its history, drop any entry
# that shares the new package's name (a rebuild of the same version replaces it), then keep newest
# first up to CANDIDATE_MAX_RETAINED. Newest-first order is preserved from the previous manifest.
history='[]'
if [[ -n "$previous" && -f "$previous" ]]; then
  history="$(jq \
    --arg current "$deb_name" \
    --argjson max "$CANDIDATE_MAX_RETAINED" \
    '([.package] + (.history // []))
       | map(select(.name != $current))
       | .[0:$max]' "$previous")"
fi

retained="$(jq 'length' <<<"$history")"
if [[ -n "$previous" && -f "$previous" && "$retained" -lt "$CANDIDATE_MIN_RETAINED" ]]; then
  echo "Note: only ${retained} previous package(s) retained; the surface builds up to ${CANDIDATE_MIN_RETAINED} over the next candidate(s)." >&2
fi

manifest="$(jq -n \
  --arg version "$version" \
  --argjson build "$build" \
  --arg commit "$commit" \
  --arg timestamp "$timestamp" \
  --arg acceptance "$acceptance" \
  --argjson package "$package" \
  --arg sums_url "$sums_url" \
  --argjson history "$history" \
  '{
    channel: "candidate",
    version: $version,
    build: $build,
    commit_sha: $commit,
    timestamp_utc: $timestamp,
    acceptance: $acceptance,
    package: $package,
    sha256sums_url: $sums_url,
    history: $history
  }')"

# Atomic publish: write to a sibling temp file, then rename over the target in one step. An
# interrupted or failed run therefore leaves the previous candidate.linux.json intact and
# discoverable — never a half-written manifest (issue #339 acceptance).
tmp="$(mktemp -- "${output}.XXXXXX")"
trap 'rm -f -- "$tmp"' EXIT
jq . <<<"$manifest" > "$tmp"
mv -f -- "$tmp" "$output"
trap - EXIT

echo "Candidate feed written: ${output}"
jq -r '"  \(.version) build \(.build) [\(.acceptance)]  \(.package.name)  \(.history | length) retained"' "$output"
