#!/usr/bin/env bash
# Run the headless non-OSC drag and single-monitor-fit regression smokes.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:?usage: run-linux-window-regression-smokes.sh <binary> <output-directory>}"
OUT_DIR="${2:?usage: run-linux-window-regression-smokes.sh <binary> <output-directory>}"
DRAG_SMOKE="${OKP_WINDOW_DRAG_SMOKE:-$ROOT/scripts/smoke-linux-window-drag.sh}"
FIT_SERIES="${OKP_WINDOW_FIT_SERIES:-$ROOT/scripts/run-linux-window-fit-series.sh}"
SOURCE_SHA="${OKP_WINDOW_REGRESSION_SOURCE_SHA:-$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)}"

[[ -x "$BINARY" ]] || { echo "Missing executable: $BINARY" >&2; exit 127; }
[[ -x "$DRAG_SMOKE" ]] || { echo "Missing executable: $DRAG_SMOKE" >&2; exit 127; }
[[ -x "$FIT_SERIES" ]] || { echo "Missing executable: $FIT_SERIES" >&2; exit 127; }

mkdir -p "$OUT_DIR"
rm -rf "$OUT_DIR/window-drag" "$OUT_DIR/window-fit"
: >"$OUT_DIR/results.tsv"

failed=0
run_smoke() {
  local name="$1" evidence="$2"
  shift 2
  local rc

  set +e
  "$@"
  rc=$?
  set -e
  if (( rc == 0 )) && [[ -s "$OUT_DIR/$evidence" ]]; then
    printf '%s\tPASS\t%s\n' "$name" "$evidence" >>"$OUT_DIR/results.tsv"
  else
    if (( rc == 0 )); then
      printf '%s\tFAIL\tmissing evidence=%s\n' "$name" "$evidence" \
        >>"$OUT_DIR/results.tsv"
    else
      printf '%s\tFAIL\texit=%s; evidence=%s\n' "$name" "$rc" "$evidence" \
        >>"$OUT_DIR/results.tsv"
    fi
    failed=1
  fi
}

run_smoke \
  non_osc_window_drag \
  window-drag/results.txt \
  "$DRAG_SMOKE" "$BINARY" "$OUT_DIR/window-drag"
run_smoke \
  single_monitor_window_fit \
  window-fit/series-evidence.txt \
  env OKP_WINDOW_FIT_SOURCE_SHA="$SOURCE_SHA" \
  "$FIT_SERIES" "$BINARY" "$OUT_DIR/window-fit"

{
  printf 'source_sha=%s\n' "$SOURCE_SHA"
  printf 'window_drag_status=%s\n' \
    "$(awk -F '\t' '$1 == "non_osc_window_drag" { print tolower($2) }' "$OUT_DIR/results.tsv")"
  printf 'window_fit_status=%s\n' \
    "$(awk -F '\t' '$1 == "single_monitor_window_fit" { print tolower($2) }' "$OUT_DIR/results.tsv")"
  if (( failed == 0 )); then
    printf 'status=pass\n'
  else
    printf 'status=fail\n'
  fi
} >"$OUT_DIR/summary.env"

(
  cd "$OUT_DIR"
  for evidence_file in \
    results.tsv \
    summary.env \
    window-drag/results.txt \
    window-drag/xvfb-evidence.txt \
    window-drag/dbus-evidence.txt \
    window-fit/series-evidence.txt; do
    [[ -f "$evidence_file" ]] || continue
    sha256sum "$evidence_file"
  done
) >"$OUT_DIR/SHA256SUMS"

if (( failed != 0 )); then
  echo "Linux window regression smokes failed. Results: $OUT_DIR/results.tsv" >&2
  exit 1
fi
echo "Linux window regression smokes passed. Results: $OUT_DIR/results.tsv"
