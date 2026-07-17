#!/usr/bin/env bash
# Promote a built Linux candidate bundle (issue #340).
#
# Build and promotion are deliberately separate: build-linux-candidate.sh
# produces a bundle and never moves a feed; this step re-validates the bundle
# and marks it ready for the candidate channel. A bundle that is not promotable
# (a failed or missing gate, a hash/identity mismatch) is rejected here too, so
# a failing build can never move the feed. Moving the actual updater feed is
# owned by #339, which consumes the same validated bundle; this script only
# performs the local, idempotent readiness gate and records the promoted SHA.
#
# It holds the same single-run lock as the builder so two invocations cannot
# publish two competing candidates.
#
# Usage: promote-linux-candidate.sh <bundle-dir>
set -euo pipefail

BUNDLE="${1:?usage: promote-linux-candidate.sh <bundle-dir>}"
SCRIPT_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
STATE_DIR="${OKP_CANDIDATE_STATE_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}/ok-player-candidate}"
LOCK="$STATE_DIR/build.lock"
LOCK_OWNER="$STATE_DIR/build.lock.owner.json"
PROMOTED="$STATE_DIR/last-promoted.sha"

RECORD="$BUNDLE/candidate-build.json"
[[ -f "$RECORD" ]] || { echo "No candidate-build.json in $BUNDLE" >&2; exit 1; }

# Resolve the validator from the bundle's own artifacts tree if present,
# otherwise from PATH.
if command -v okp-candidate >/dev/null 2>&1; then
  CANDIDATE_CLI="okp-candidate"
elif [[ -x "$STATE_DIR/checkout/rust/target/release/okp-candidate" ]]; then
  CANDIDATE_CLI="$STATE_DIR/checkout/rust/target/release/okp-candidate"
else
  echo "okp-candidate binary not found (build a candidate first)" >&2
  exit 1
fi

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
    --phase promote \
    --source-sha "$(jq -r '.source_sha' "$RECORD")" \
    -- "$0" "$@"
fi

# Recompute the bundle's package/checksum/Velopack identities immediately
# before promotion. A post-build byte replacement must never advance the
# marker or reach the rolling publisher.
"$CANDIDATE_CLI" verify-bundle --bundle "$BUNDLE"

SHA="$(jq -r '.source_sha' "$RECORD")"
[[ "$SHA" =~ ^[0-9a-f]{40}$ ]] || { echo "could not read source_sha from $RECORD" >&2; exit 1; }

echo "$SHA" >"$PROMOTED"
echo "Promoted candidate for source ${SHA} (bundle ${BUNDLE})."
echo "Feed movement is owned by #339, which consumes this validated bundle."
