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
  for required in \
    'session_bus_ready=true' \
    'session_bus_teardown=clean' \
    'session_process_teardown=clean' \
    'command_status=0' \
    'status=pass'; do
    if ! rg -Fx -q "$required" "$run_dir/fit-session-evidence.txt"; then
      printf 'failed_run=%s\nmissing_session_evidence=%s\nstatus=fail\n' \
        "$run" "$required" >>"$OUT_DIR/series-evidence.txt"
      echo "Window-fit run $run did not prove isolated D-Bus teardown: $required" >&2
      exit 1
    fi
  done
  for required in \
    'xdg_cache_home_isolated=true' \
    'xdg_runtime_dir_isolated=true' \
    'xdg_runtime_mode=700' \
    'accessibility_disabled=true'; do
    if ! rg -Fx -q "$required" "$run_dir/fit-evidence.txt"; then
      printf 'failed_run=%s\nmissing_namespace_evidence=%s\nstatus=fail\n' \
        "$run" "$required" >>"$OUT_DIR/series-evidence.txt"
      echo "Window-fit run $run did not prove private GTK namespaces: $required" >&2
      exit 1
    fi
  done
  for required in \
    'xvfb_ready=true' \
    'xvfb_alive_before_teardown=true' \
    'xvfb_teardown=clean' \
    'command_status=0' \
    'status=pass'; do
    if ! rg -Fx -q "$required" "$run_dir/fit-xvfb-evidence.txt"; then
      printf 'failed_run=%s\nmissing_xvfb_evidence=%s\nstatus=fail\n' \
        "$run" "$required" >>"$OUT_DIR/series-evidence.txt"
      echo "Window-fit run $run did not prove isolated Xvfb teardown: $required" >&2
      exit 1
    fi
  done
  {
    printf 'run=%s begin\n' "$run"
    sed "s/^/run=${run} /" "$run_dir/fit-evidence.txt"
    printf 'run=%s end\n' "$run"
  } >>"$OUT_DIR/series-evidence.txt"
done

printf 'completed_consecutive_runs=3\nstatus=pass\n' >>"$OUT_DIR/series-evidence.txt"
echo "Window-fit smoke passed three consecutive runs. Evidence: $OUT_DIR/series-evidence.txt"
