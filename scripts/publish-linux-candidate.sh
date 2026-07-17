#!/usr/bin/env bash
# Publish one already-built native Linux candidate bundle (issue #339).
# The bundle is never rebuilt here. okp-core verifies all identities and
# assembles the manifest; this script only performs the ordered GitHub asset
# operations and advances the local marker after the pointer is live.
set -euo pipefail

BUNDLE="${1:?usage: publish-linux-candidate.sh <bundle-dir> <owner/repo> [acceptance]}"
REPO="${2:?usage: publish-linux-candidate.sh <bundle-dir> <owner/repo> [acceptance]}"
ACCEPTANCE="${3:-accepted}"
STATE_DIR="${OKP_CANDIDATE_STATE_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}/ok-player-candidate}"
TAG="${OKP_CANDIDATE_TAG:-linux-candidate}"
LOCK="$STATE_DIR/build.lock"
PROMOTED="$STATE_DIR/last-promoted.sha"
CLI="${OKP_CANDIDATE_CLI:-$STATE_DIR/checkout/rust/target/release/okp-candidate}"

[[ -x "$CLI" ]] || { echo "okp-candidate binary not found: $CLI" >&2; exit 1; }
[[ -f "$BUNDLE/candidate-build.json" ]] || { echo "candidate-build.json missing from $BUNDLE" >&2; exit 1; }
case "$ACCEPTANCE" in
  pending|accepted|rejected) ;;
  *) echo "acceptance must be pending, accepted, or rejected" >&2; exit 1 ;;
esac

mkdir -p "$STATE_DIR"
exec 9>"$LOCK"
flock -n 9 || { echo "another candidate build/publish holds the lock" >&2; exit 1; }

"$CLI" verify-bundle --bundle "$BUNDLE"

work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT
previous="$work/previous.candidate.linux.json"
feed="$work/candidate.linux.json"
assets_json="$work/assets.json"
prune_plan="$work/prune-plan.txt"

if ! gh release view "$TAG" --repo "$REPO" >/dev/null 2>&1; then
  gh release create "$TAG" --repo "$REPO" --prerelease \
    --title "OK Player Linux candidate (rolling)" \
    --notes "Mutable rolling QA candidate. Assets are replaced in place; this is not a permanent product release."
fi
if ! gh release download "$TAG" --repo "$REPO" --pattern candidate.linux.json \
  --output "$previous" --clobber 2>/dev/null; then
  rm -f -- "$previous"
fi

previous_args=()
if [[ -s "$previous" ]]; then
  previous_args=(--previous "$previous")
fi
base_url="https://github.com/${REPO}/releases/download/${TAG}"
"$CLI" feed --bundle "$BUNDLE" --base-url "$base_url" \
  --acceptance "$ACCEPTANCE" "${previous_args[@]}" --output "$feed"

version="$(jq -r '.version' "$BUNDLE/candidate-build.json")"
build="$(jq -r '.build_number' "$BUNDLE/candidate-build.json")"
source_sha="$(jq -r '.source_sha' "$BUNDLE/candidate-build.json")"
deb_name="$(jq -r '.package.artifacts[] | select(.kind == "debian") | .file_name' "$BUNDLE/candidate-build.json")"
appimage_name="$(jq -r '.package.artifacts[] | select(.kind == "app-image") | .file_name' "$BUNDLE/candidate-build.json")"
velopack_name="$(jq -r '.appimage.name' "$feed")"
sums_name="SHA256SUMS-${build}.txt"
cp "$BUNDLE/artifacts/SHA256SUMS" "$work/$sums_name"

# Upload immutable/versioned bytes first. candidate.linux.json is the single
# accepted pointer for both lanes and is deliberately uploaded last.
gh release upload "$TAG" --repo "$REPO" "$BUNDLE/artifacts/deb/$deb_name" --clobber
gh release upload "$TAG" --repo "$REPO" "$BUNDLE/artifacts/velopack/$appimage_name" --clobber
gh release upload "$TAG" --repo "$REPO" "$BUNDLE/artifacts/velopack/$velopack_name" --clobber
gh release upload "$TAG" --repo "$REPO" "$work/$sums_name" --clobber
gh release upload "$TAG" --repo "$REPO" "$feed" --clobber

# Bound the mutable surface only after the new pointer is usable. The core
# decides which recognized candidate assets are outside current + history.
gh release view "$TAG" --repo "$REPO" --json assets --jq '[.assets[].name]' >"$assets_json"
"$CLI" prune-plan --feed "$feed" --assets "$assets_json" >"$prune_plan"
while IFS= read -r asset; do
  [[ -n "$asset" ]] || continue
  gh release delete-asset "$TAG" "$asset" --repo "$REPO" --yes
done <"$prune_plan"

if [[ "$ACCEPTANCE" == "accepted" ]]; then
  marker_tmp="$(mktemp -- "$STATE_DIR/last-promoted.sha.XXXXXX")"
  printf '%s\n' "$source_sha" >"$marker_tmp"
  mv -f -- "$marker_tmp" "$PROMOTED"
fi

echo "Published ${version} build ${build} (${source_sha}) to rolling candidate ${TAG}."
