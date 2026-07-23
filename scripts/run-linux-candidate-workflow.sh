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
DECISION_OUTPUT="$STATE_DIR/last-publish-decision.json"
DECISION_REACHED=false
export OKP_PORTABILITY_EVIDENCE_DIR="${OKP_PORTABILITY_EVIDENCE_DIR:-$ROOT/artifacts/linux/portability-smoke-evidence}"
mkdir -p "$OKP_PORTABILITY_EVIDENCE_DIR"

"$ROOT/scripts/build-linux-candidate.sh"

BUNDLE="$(cat "$STATE_DIR/last-bundle.path")"
SOURCE_SHA="$(jq -r '.source_sha' "$BUNDLE/candidate-build.json")"
PROMOTED_SHA="$(cat "$STATE_DIR/last-promoted.sha" 2>/dev/null || true)"
SHOULD_PUBLISH=false
PUBLISH_RESULT=skipped
DELIVERY_RESULT=already_delivered
if [[ "$SOURCE_SHA" != "$PROMOTED_SHA" || "$FORCE_REPUBLISH" == "true" ]]; then
  SHOULD_PUBLISH=true
  OKP_CANDIDATE_PUBLISH_DECISION="$DECISION_OUTPUT" \
    "$STATE_DIR/checkout/scripts/publish-linux-candidate.sh" \
    "$BUNDLE" "$REPO" "$ACCEPTANCE"
  DECISION_REACHED=true
  case "$(jq -r '.outcome' "$DECISION_OUTPUT")" in
    eligible)
      PUBLISH_RESULT=published
      DELIVERY_RESULT=delivered
      ;;
    stale_generation)
      PUBLISH_RESULT=stale_generation
      DELIVERY_RESULT=non_delivery
      ;;
    *) echo "candidate publisher returned invalid decision evidence" >&2; exit 1 ;;
  esac
fi

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  {
    echo "bundle=$BUNDLE"
    echo "source_sha=$SOURCE_SHA"
    echo "acceptance=$ACCEPTANCE"
    echo "should_publish=$SHOULD_PUBLISH"
    echo "publish_result=$PUBLISH_RESULT"
    echo "delivery_result=$DELIVERY_RESULT"
    if [[ "$DECISION_REACHED" == "true" && -s "$DECISION_OUTPUT" ]]; then
      echo "requested_sha=$(jq -r '.requested_sha' "$DECISION_OUTPUT")"
      echo "build_sha=$(jq -r '.build_sha' "$DECISION_OUTPUT")"
      echo "current_sha=$(jq -r '.current_sha' "$DECISION_OUTPUT")"
      echo "build_number=$(jq -r '.build_number' "$DECISION_OUTPUT")"
      echo "allocated_build=$(jq -r '.allocated_build' "$DECISION_OUTPUT")"
      published_build="$(jq -r '.published_build // "none"' "$DECISION_OUTPUT")"
      echo "published_build=$published_build"
      echo "stale_reasons=$(jq -c '.stale_reasons // []' "$DECISION_OUTPUT")"
    fi
  } >> "$GITHUB_OUTPUT"
fi
