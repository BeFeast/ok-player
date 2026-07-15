#!/usr/bin/env bash
# Merge deterministic harness rows into the package-bound operator template.
set -euo pipefail

TEMPLATE="${1:?acceptance template is required}"
XVFB_ROWS="${2:?xvfb rows are required}"
OUT="${3:?output manifest is required}"

command -v jq >/dev/null 2>&1 || { echo "Missing required tool: jq" >&2; exit 127; }

jq --slurpfile xvfb "$XVFB_ROWS" '
  .rows |= map(
    . as $existing
    | (($xvfb[0] | map(select(.state == $existing.state))) | first) // $existing
  )
' "$TEMPLATE" >"$OUT"

echo "Merged deterministic evidence into $OUT"
