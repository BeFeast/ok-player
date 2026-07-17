#!/usr/bin/env bash
# One locked build -> exact-bundle handoff -> publish transaction for the
# rolling Linux candidate workflow. The caller must own the candidate lock.
set -euo pipefail

[[ "${OKP_CANDIDATE_LOCK_HELD:-}" == "1" ]] || {
  echo "run-linux-candidate-workflow.sh requires the candidate lock" >&2
  exit 1
}

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
STATE_DIR="${OKP_CANDIDATE_STATE_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}/ok-player-candidate}"
REPO="${OKP_CANDIDATE_REPOSITORY:?OKP_CANDIDATE_REPOSITORY is required}"
ACCEPTANCE="${OKP_CANDIDATE_ACCEPTANCE:-accepted}"
FORCE_REPUBLISH="${OKP_CANDIDATE_FORCE_REPUBLISH:-false}"

"$ROOT/scripts/build-linux-candidate.sh"

BUNDLE="$(cat "$STATE_DIR/last-bundle.path")"
SOURCE_SHA="$(jq -r '.source_sha' "$BUNDLE/candidate-build.json")"
PROMOTED_SHA="$(cat "$STATE_DIR/last-promoted.sha" 2>/dev/null || true)"
SHOULD_PUBLISH=false
PUBLISH_RESULT=skipped
if [[ "$SOURCE_SHA" != "$PROMOTED_SHA" || "$FORCE_REPUBLISH" == "true" ]]; then
  SHOULD_PUBLISH=true
  "$STATE_DIR/checkout/scripts/publish-linux-candidate.sh" \
    "$BUNDLE" "$REPO" "$ACCEPTANCE"
  PUBLISH_RESULT=published
fi

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  {
    echo "bundle=$BUNDLE"
    echo "source_sha=$SOURCE_SHA"
    echo "acceptance=$ACCEPTANCE"
    echo "should_publish=$SHOULD_PUBLISH"
    echo "publish_result=$PUBLISH_RESULT"
  } >> "$GITHUB_OUTPUT"
fi
