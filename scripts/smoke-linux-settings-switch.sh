#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-settings-switch-smoke}"

for theme in light dark; do
  for scale in 100 125 150 200; do
    "$ROOT/scripts/smoke-linux-settings.sh" \
      "$BINARY" \
      "$OUT_DIR/${theme}-${scale}" \
      video \
      "$theme" \
      "" \
      "$scale"
  done
done

echo "Settings switch scale matrix passed (light/dark at 100%, 125%, 150%, and 200%)."
