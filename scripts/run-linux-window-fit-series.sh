#!/usr/bin/env bash
# Run the complete Xvfb window-fit smoke three times without retrying a failed run.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:?usage: run-linux-window-fit-series.sh <binary> <output-directory>}"
OUT_DIR="${2:?usage: run-linux-window-fit-series.sh <binary> <output-directory>}"
SOURCE_SHA="${OKP_WINDOW_FIT_SOURCE_SHA:-$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)}"

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"
: >"$OUT_DIR/series-evidence.txt"
printf 'source_sha=%s\nrequired_consecutive_runs=3\n' "$SOURCE_SHA" \
  >>"$OUT_DIR/series-evidence.txt"

for run in 1 2 3; do
  run_dir="$OUT_DIR/run-$run"
  if ! OKP_MAIN_WINDOW_FIT_ONLY=1 \
    "$ROOT/scripts/smoke-linux-main-window.sh" "$BINARY" "$run_dir"; then
    printf 'failed_run=%s\nstatus=fail\n' "$run" >>"$OUT_DIR/series-evidence.txt"
    echo "Window-fit consecutive series failed at run $run; restart the series from zero." >&2
    exit 1
  fi
  {
    printf 'run=%s begin\n' "$run"
    sed "s/^/run=${run} /" "$run_dir/fit-evidence.txt"
    printf 'run=%s end\n' "$run"
  } >>"$OUT_DIR/series-evidence.txt"
done

printf 'completed_consecutive_runs=3\nstatus=pass\n' >>"$OUT_DIR/series-evidence.txt"
echo "Window-fit smoke passed three consecutive runs. Evidence: $OUT_DIR/series-evidence.txt"
