#!/usr/bin/env bash
# Merge deterministic harness rows into the package-bound operator template.
set -euo pipefail

TEMPLATE="${1:?acceptance template is required}"
XVFB_ROWS="${2:?xvfb rows are required}"
OUT="${3:?output manifest is required}"

command -v jq >/dev/null 2>&1 || { echo "Missing required tool: jq" >&2; exit 127; }

# The output is allowed to be the template: a release merges the live GNOME/Wayland rows
# back into the manifest the deterministic pass already produced. Redirecting straight at
# "$OUT" would truncate that file before jq opened it, and jq accepts empty input - the
# merge would report success and leave a zero-byte manifest behind. Stage the result
# beside the destination and rename it into place instead.
tmp="$(mktemp "$OUT.XXXXXX")"
trap 'rm -f "$tmp"' EXIT

jq --slurpfile xvfb "$XVFB_ROWS" '
  .rows |= map(
    . as $existing
    | (($xvfb[0] | map(select(.state == $existing.state))) | first) // $existing
  )
' "$TEMPLATE" >"$tmp"

mv "$tmp" "$OUT"

echo "Merged deterministic evidence into $OUT"
