#!/usr/bin/env bash
# Remove only scratch roots attributed to one worker or workflow session.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT/scripts/ok-player-scratch.sh"

SESSION="${1:-${OKP_SCRATCH_SESSION:-}}"
[[ -n "$SESSION" ]] || {
  echo "usage: reclaim-ok-player-scratch.sh <session-key>" >&2
  exit 2
}
okp_validate_scratch_session "$SESSION"

SCRATCH_ROOT="${OKP_SCRATCH_ROOT:-${TMPDIR:-/tmp}}"
[[ -d "$SCRATCH_ROOT" ]] || exit 0
CURRENT_UID="$(id -u)"
status=0
shopt -s nullglob
for path in \
  "$SCRATCH_ROOT"/"ok-player-${SESSION}-"* \
  "$SCRATCH_ROOT"/"okp-${SESSION}-"*; do
  [[ -e "$path" || -L "$path" ]] || continue
  if [[ "$(stat -c %u -- "$path")" != "$CURRENT_UID" ]]; then
    echo "refusing to reclaim scratch not owned by the current user: $(basename -- "$path")" >&2
    status=1
    continue
  fi
  rm -rf -- "$path"
done
exit "$status"
