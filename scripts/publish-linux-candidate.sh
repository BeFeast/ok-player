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
SCRIPT_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
source "$SCRIPT_ROOT/scripts/ok-player-scratch.sh"
TAG="${OKP_CANDIDATE_TAG:-linux-candidate}"
BRANCH="${OKP_CANDIDATE_BRANCH:-main}"
REQUESTED_SHA="${OKP_CANDIDATE_REQUESTED_SHA:?OKP_CANDIDATE_REQUESTED_SHA is required}"
LOCK="$STATE_DIR/build.lock"
LOCK_OWNER="$STATE_DIR/build.lock.owner.json"
PROMOTED="$STATE_DIR/last-promoted.sha"
BUILD_NUMBER_FILE="$STATE_DIR/build-number"
DECISION_OUTPUT="${OKP_CANDIDATE_PUBLISH_DECISION:-$STATE_DIR/last-publish-decision.json}"
CLI="${OKP_CANDIDATE_CLI:-$STATE_DIR/checkout/rust/target/release/okp-candidate}"
VISIBILITY_ATTEMPTS="${OKP_CANDIDATE_VISIBILITY_ATTEMPTS:-120}"
VISIBILITY_INTERVAL_SECONDS="${OKP_CANDIDATE_VISIBILITY_INTERVAL_SECONDS:-5}"

[[ -x "$CLI" ]] || { echo "okp-candidate binary not found: $CLI" >&2; exit 1; }
[[ -f "$BUNDLE/candidate-build.json" ]] || { echo "candidate-build.json missing from $BUNDLE" >&2; exit 1; }
[[ -s "$BUILD_NUMBER_FILE" ]] || { echo "candidate build-number state is missing" >&2; exit 1; }
[[ "$VISIBILITY_ATTEMPTS" =~ ^[1-9][0-9]*$ ]] || {
  echo "OKP_CANDIDATE_VISIBILITY_ATTEMPTS must be a positive integer" >&2
  exit 1
}
[[ "$VISIBILITY_INTERVAL_SECONDS" =~ ^[0-9]+$ ]] || {
  echo "OKP_CANDIDATE_VISIBILITY_INTERVAL_SECONDS must be a non-negative integer" >&2
  exit 1
}
case "$ACCEPTANCE" in
  pending|accepted|rejected) ;;
  *) echo "acceptance must be pending, accepted, or rejected" >&2; exit 1 ;;
esac

mkdir -p "$STATE_DIR"
if [[ "${OKP_CANDIDATE_LOCK_HELD:-}" != "1" ]]; then
  if [[ -n "${OKP_CANDIDATE_LOCK_CLI:-}" ]]; then
    LOCK_CLI="$OKP_CANDIDATE_LOCK_CLI"
  else
    CC="${CC:-/usr/bin/cc}" cargo build --quiet \
      --manifest-path "$SCRIPT_ROOT/rust/Cargo.toml" \
      -p okp-core --bin okp-candidate
    LOCK_CLI="$SCRIPT_ROOT/rust/target/debug/okp-candidate"
  fi
  [[ -x "$LOCK_CLI" ]] || { echo "candidate lock coordinator not found: $LOCK_CLI" >&2; exit 1; }
  exec "$LOCK_CLI" lock-run \
    --lock "$LOCK" \
    --owner "$LOCK_OWNER" \
    --phase publish \
    --source-sha "$(jq -r '.source_sha' "$BUNDLE/candidate-build.json")" \
    -- "$0" "$@"
fi

"$CLI" verify-bundle --bundle "$BUNDLE"

work="$(okp_make_scratch_dir candidate-publish)"
previous_dir="$work/previous"
existing_dir="$work/existing"
mkdir -p "$previous_dir" "$existing_dir"
previous="$previous_dir/candidate.linux.json"
feed="$work/candidate.linux.json"
assets_json="$work/assets.json"
preexisting_assets="$work/preexisting-assets.json"
prune_plan="$work/prune-plan.txt"
pointer_attempted=false
pointer_committed=false
expected_assets=()
release_exists=false

cleanup() {
  local status="$?"
  trap - EXIT
  if [[ "$status" -ne 0 && "$pointer_committed" != "true" ]]; then
    if [[ "$pointer_attempted" == "true" ]]; then
      if [[ -s "$previous" ]]; then
        gh release upload "$TAG" --repo "$REPO" "$previous" --clobber >/dev/null 2>&1 \
          || echo "warning: failed to restore the previous candidate pointer" >&2
      else
        gh release delete-asset "$TAG" candidate.linux.json --repo "$REPO" --yes \
          >/dev/null 2>&1 || true
      fi
    fi
    for asset in "${expected_assets[@]}"; do
      if ! jq -e --arg name "$asset" 'index($name) != null' "$preexisting_assets" \
        >/dev/null 2>&1; then
        gh release delete-asset "$TAG" "$asset" --repo "$REPO" --yes \
          >/dev/null 2>&1 || true
      fi
    done
  fi
  rm -rf -- "$work"
  exit "$status"
}
trap cleanup EXIT

if gh release view "$TAG" --repo "$REPO" >/dev/null 2>&1; then
  release_exists=true
  gh release view "$TAG" --repo "$REPO" --json assets --jq '[.assets[].name]' \
    >"$preexisting_assets"
  if jq -e 'index("candidate.linux.json") != null' "$preexisting_assets" >/dev/null; then
    gh release download "$TAG" --repo "$REPO" --pattern candidate.linux.json \
      --dir "$previous_dir" --clobber || {
        echo "failed to preserve the existing candidate pointer before publication" >&2
        exit 1
      }
  else
    rm -f -- "$previous"
  fi
else
  printf '[]\n' >"$preexisting_assets"
  rm -f -- "$previous"
fi

version="$(jq -r '.version' "$BUNDLE/candidate-build.json")"
build="$(jq -r '.build_number' "$BUNDLE/candidate-build.json")"
source_sha="$(jq -r '.source_sha' "$BUNDLE/candidate-build.json")"
allocated_build="$(cat "$BUILD_NUMBER_FILE")"
current_sha="$(gh api "repos/${REPO}/git/ref/heads/${BRANCH}" --jq '.object.sha')"

decision_args=()
if [[ -s "$previous" ]]; then
  decision_args=(--published-feed "$previous")
fi
decision="$work/publish-decision.json"
"$CLI" publish-decision \
  --requested-sha "$REQUESTED_SHA" \
  --build-sha "$source_sha" \
  --current-sha "$current_sha" \
  --build-number "$build" \
  --allocated-build "$allocated_build" \
  "${decision_args[@]}" >"$decision"

decision_parent="$(dirname -- "$DECISION_OUTPUT")"
mkdir -p "$decision_parent"
decision_tmp="$(mktemp -- "$decision_parent/.publish-decision.XXXXXX")"
cp "$decision" "$decision_tmp"
mv -f -- "$decision_tmp" "$DECISION_OUTPUT"

if [[ "$(jq -r '.outcome' "$decision")" == "stale_generation" ]]; then
  echo "Candidate publication is a stale_generation no-op: $(jq -c . "$decision")"
  exit 0
fi

if [[ "$release_exists" != "true" ]]; then
  gh release create "$TAG" --repo "$REPO" --prerelease \
    --title "OK Player Linux candidate (rolling)" \
    --notes "Mutable rolling QA candidate. Assets are replaced in place; this is not a permanent product release."
fi

previous_args=()
if [[ -s "$previous" ]]; then
  previous_args=(--previous "$previous")
fi
base_url="https://github.com/${REPO}/releases/download/${TAG}"
"$CLI" feed --bundle "$BUNDLE" --base-url "$base_url" \
  --acceptance "$ACCEPTANCE" "${previous_args[@]}" --output "$feed"

deb_name="$(jq -r '.package.artifacts[] | select(.kind == "debian") | .file_name' "$BUNDLE/candidate-build.json")"
appimage_name="$(jq -r '.package.artifacts[] | select(.kind == "app-image") | .file_name' "$BUNDLE/candidate-build.json")"
velopack_name="$(jq -r '.appimage.name' "$feed")"
sums_name="SHA256SUMS-${build}.txt"
cp "$BUNDLE/artifacts/SHA256SUMS" "$work/$sums_name"
expected_assets=("$deb_name" "$appimage_name" "$velopack_name" "$sums_name")

upload_exact_asset() {
  local source="$1" name="$2"
  if jq -e --arg name "$name" 'index($name) != null' "$preexisting_assets" >/dev/null; then
    rm -f -- "$existing_dir/$name"
    gh release download "$TAG" --repo "$REPO" --pattern "$name" \
      --dir "$existing_dir" --clobber >/dev/null
    cmp -s -- "$source" "$existing_dir/$name" || {
      echo "existing candidate asset $name does not match the verified bundle" >&2
      return 1
    }
    echo "Reusing verified candidate asset $name."
  else
    gh release upload "$TAG" --repo "$REPO" "$source"
  fi
}

wait_for_pointer_visibility() {
  local expected="$1" url="$2" observed="$work/visible-candidate.linux.json"
  local attempt observed_identity
  for ((attempt = 1; attempt <= VISIBILITY_ATTEMPTS; attempt++)); do
    if curl --fail --silent --show-error --location \
      --connect-timeout 10 --max-time 30 \
      --header "Accept: application/json" \
      --user-agent "OK Player Linux" \
      --output "$observed" "$url"; then
      if cmp -s -- "$expected" "$observed"; then
        echo "Canonical candidate pointer is visible after attempt ${attempt}."
        return 0
      fi
      observed_identity="$(jq -c '{version, build, commit_sha, acceptance}' "$observed" 2>/dev/null || printf 'invalid-json')"
      echo "Canonical candidate pointer is stale on attempt ${attempt}: ${observed_identity}" >&2
    else
      echo "Canonical candidate pointer could not be fetched on attempt ${attempt}." >&2
    fi
    if ((attempt < VISIBILITY_ATTEMPTS)); then
      sleep "$VISIBILITY_INTERVAL_SECONDS"
    fi
  done
  echo "candidate pointer was uploaded but the canonical URL did not expose its exact bytes" >&2
  return 1
}

# Upload immutable/versioned bytes first. candidate.linux.json is the single
# accepted pointer for both lanes and is deliberately uploaded last.
upload_exact_asset "$BUNDLE/artifacts/deb/$deb_name" "$deb_name"
upload_exact_asset "$BUNDLE/artifacts/velopack/$appimage_name" "$appimage_name"
upload_exact_asset "$BUNDLE/artifacts/velopack/$velopack_name" "$velopack_name"
upload_exact_asset "$work/$sums_name" "$sums_name"
pointer_attempted=true
gh release upload "$TAG" --repo "$REPO" "$feed" --clobber
pointer_committed=true
candidate_url="${OKP_CANDIDATE_PUBLIC_URL:-$base_url/candidate.linux.json}"
wait_for_pointer_visibility "$feed" "$candidate_url"

# Bound the mutable surface only after the new pointer is usable. The core
# decides which recognized candidate assets are outside current + history.
# Cleanup is post-commit maintenance: a pruning failure must not report the
# already-live pointer as a failed publication.
if gh release view "$TAG" --repo "$REPO" --json assets --jq '[.assets[].name]' >"$assets_json" \
  && "$CLI" prune-plan --feed "$feed" --assets "$assets_json" >"$prune_plan"; then
  while IFS= read -r asset; do
    [[ -n "$asset" ]] || continue
    gh release delete-asset "$TAG" "$asset" --repo "$REPO" --yes \
      || echo "warning: failed to prune candidate asset $asset" >&2
  done <"$prune_plan"
else
  echo "warning: failed to calculate candidate asset pruning after publication" >&2
fi

if [[ "$ACCEPTANCE" == "accepted" ]]; then
  marker_tmp="$(mktemp -- "$STATE_DIR/last-promoted.sha.XXXXXX")"
  printf '%s\n' "$source_sha" >"$marker_tmp"
  if ! mv -f -- "$marker_tmp" "$PROMOTED"; then
    rm -f -- "$marker_tmp"
    echo "warning: candidate is live but the local promoted marker was not updated" >&2
  fi
fi

echo "Published ${version} build ${build} (${source_sha}) to rolling candidate ${TAG}."
